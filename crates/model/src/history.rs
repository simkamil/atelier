// SPDX-License-Identifier: AGPL-3.0-or-later
//! In-memory, generic undo/redo history for reversible model transactions.

use core::fmt;
use std::collections::VecDeque;

use crate::{
    MAX_TRANSACTION_OPERATIONS, Operation, Transaction, TransactionError, TransactionalState,
};

const TYPING_GROUP_PAUSE_MS: u64 = 1_000;

/// The edit category supplied by the caller to control undo grouping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditKind {
    /// A typing edit. `segment` changes when the caller considers the typing
    /// context different. Set `closes_after` for a Unicode whitespace after a
    /// word: that whitespace joins the current group and closes it afterward.
    Typing { segment: u64, closes_after: bool },
    /// Any non-typing edit forms its own group and closes adjacent typing.
    Other,
}

/// Which history operation was applying a transaction when it failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryAction {
    Apply,
    Undo,
    Redo,
}

/// Failure to record or navigate a history entry.
#[derive(Debug, PartialEq, Eq)]
pub enum HistoryError<E> {
    /// The caller-declared upper bound cannot fit even as the sole entry.
    BudgetExceeded { budget: usize, required: usize },
    /// A checked cost calculation overflowed `usize`.
    CostOverflow,
    /// The history's bounded containers could not reserve their next slot.
    StorageReservationFailed,
    /// The underlying transaction failed. Its original operation error and index
    /// remain available in `source`.
    Transaction {
        action: HistoryAction,
        source: TransactionError<E>,
    },
}

impl<E: fmt::Display> fmt::Display for HistoryError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded { budget, required } => write!(
                f,
                "history entry requires {required} bytes, exceeding budget {budget}"
            ),
            Self::CostOverflow => f.write_str("history cost calculation overflowed"),
            Self::StorageReservationFailed => f.write_str("could not reserve history storage"),
            Self::Transaction { action, source } => {
                write!(f, "{action:?} transaction failed: {source}")
            }
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for HistoryError<E> {}

struct TypingGroup {
    segment: u64,
    last_ms: u64,
    closed_after: bool,
}

struct Entry<O, Selection> {
    /// This is the one direction currently needed to move away from this entry.
    /// A successful navigation replaces it with the inverse returned by apply.
    transaction: Transaction<O>,
    before: Selection,
    after: Selection,
    cost_bound: usize,
    typing: Option<TypingGroup>,
}

/// An in-memory undo/redo controller. `Selection` is deliberately opaque to the
/// model and only needs to be clonable so snapshots remain owned by history.
pub struct History<O, Selection> {
    undo: VecDeque<Entry<O, Selection>>,
    redo: VecDeque<Entry<O, Selection>>,
    current_selection: Selection,
    budget_bytes: usize,
}

impl<O, Selection: Clone> History<O, Selection> {
    /// Creates an empty history with the caller's initial selection and logical
    /// memory budget. The budget is the caller's declared conservative bound,
    /// not a measurement of Rust allocator usage.
    pub fn new(initial_selection: Selection, budget_bytes: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            current_selection: initial_selection,
            budget_bytes,
        }
    }

    /// Returns the currently restored selection snapshot.
    pub fn selection(&self) -> &Selection {
        &self.current_selection
    }

    /// Returns the configured logical budget.
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// Returns the declared bounds of entries in both stacks.
    pub fn retained_cost_bound(&self) -> Result<usize, HistoryError<core::convert::Infallible>> {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .try_fold(0usize, |sum, entry| {
                sum.checked_add(entry.cost_bound)
                    .ok_or(HistoryError::CostOverflow)
            })
    }

    /// Updates the current selection after explicit cursor movement and closes
    /// the active typing group. Cursor movement itself is not an edit entry.
    pub fn move_cursor(&mut self, selection: Selection) {
        self.current_selection = selection;
        if let Some(last) = self.undo.back_mut() {
            last.typing = None;
        }
    }

    /// Applies a non-empty transaction and records its inverse. `cost_bound`
    /// must conservatively cover the maximum future direction(s), both retained
    /// snapshots, logical entry/stack overhead, and any operation/inverse costs.
    /// It is checked before applying the transaction.
    pub fn apply<S>(
        &mut self,
        state: &mut S,
        transaction: Transaction<O>,
        after: Selection,
        now_ms: u64,
        kind: EditKind,
        cost_bound: usize,
    ) -> Result<(), HistoryError<O::Error>>
    where
        S: TransactionalState,
        O: Operation<S>,
    {
        if transaction.operations().is_empty() {
            return Ok(());
        }
        let cost_bound = cost_bound.max(minimum_entry_cost::<O, Selection>());
        if cost_bound > self.budget_bytes {
            return Err(HistoryError::BudgetExceeded {
                budget: self.budget_bytes,
                required: cost_bound,
            });
        }

        // A group is possible only when both adjacent caller events are typing,
        // in the same segment, within the inclusive 1,000 ms boundary, and the
        // previous event did not close its group.
        let can_group = match (self.undo.back(), kind) {
            (Some(previous), EditKind::Typing { segment, .. }) => {
                previous.typing.as_ref().is_some_and(|group| {
                    group.segment == segment
                        && !group.closed_after
                        && now_ms
                            .checked_sub(group.last_ms)
                            .is_some_and(|gap| gap <= TYPING_GROUP_PAUSE_MS)
                })
            }
            _ => false,
        };
        let grouped_operation_count = if can_group {
            self.undo
                .back()
                .map(|entry| entry.transaction.operations().len())
                .unwrap_or(0)
                .checked_add(transaction.operations().len())
                .filter(|count| *count <= MAX_TRANSACTION_OPERATIONS)
        } else {
            None
        };
        let grouped_cost = if grouped_operation_count.is_some() {
            Some(
                self.undo
                    .back()
                    .expect("group candidate exists")
                    .cost_bound
                    .checked_add(cost_bound)
                    .ok_or(HistoryError::CostOverflow)?,
            )
        } else {
            None
        };
        let group = grouped_cost.filter(|cost| *cost <= self.budget_bytes);

        let mut undo_cost = self.undo.iter().try_fold(0usize, |sum, entry| {
            sum.checked_add(entry.cost_bound)
                .ok_or(HistoryError::CostOverflow)
        })?;
        // A successful new edit clears redo, so redo costs are intentionally not
        // part of the post-commit reservation plan.
        let mut evict_count = 0usize;
        if group.is_some() {
            undo_cost = undo_cost
                .checked_add(cost_bound)
                .ok_or(HistoryError::CostOverflow)?;
            let protected_last = self.undo.len() - 1;
            for entry in self.undo.iter().take(protected_last) {
                if undo_cost <= self.budget_bytes {
                    break;
                }
                undo_cost -= entry.cost_bound;
                evict_count += 1;
            }
        } else {
            undo_cost = undo_cost
                .checked_add(cost_bound)
                .ok_or(HistoryError::CostOverflow)?;
            for entry in &self.undo {
                if undo_cost <= self.budget_bytes {
                    break;
                }
                undo_cost -= entry.cost_bound;
                evict_count += 1;
            }
        }
        // The new standalone entry fits by the check above; the planned oldest
        // evictions must therefore be sufficient.
        if undo_cost > self.budget_bytes {
            return Err(HistoryError::BudgetExceeded {
                budget: self.budget_bytes,
                required: undo_cost,
            });
        }

        if group.is_none() {
            self.undo
                .try_reserve(1)
                .map_err(|_| HistoryError::StorageReservationFailed)?;
        }
        let mut combined_operations = if group.is_some() {
            let count = grouped_operation_count.expect("grouped count exists");
            let mut operations = Vec::new();
            operations
                .try_reserve_exact(count)
                .map_err(|_| HistoryError::StorageReservationFailed)?;
            Some(operations)
        } else {
            None
        };

        // No history or selection mutation occurs until the model transaction
        // has succeeded. Transaction::apply restores its exact checkpoint on
        // either inversion or application failure.
        let inverse = transaction
            .apply(state)
            .map_err(|source| HistoryError::Transaction {
                action: HistoryAction::Apply,
                source,
            })?;

        for _ in 0..evict_count {
            self.undo.pop_front();
        }
        self.redo.clear();
        let before = self.current_selection.clone();
        self.current_selection = after.clone();
        match group {
            Some(cost) => {
                let mut previous = self.undo.pop_back().expect("group candidate exists");
                let operations = combined_operations
                    .as_mut()
                    .expect("group operation buffer exists");
                operations.extend(inverse.into_operations());
                operations.extend(previous.transaction.into_operations());
                previous.transaction = Transaction::new(core::mem::take(operations))
                    .expect("group operation count was checked before apply");
                previous.after = after;
                previous.cost_bound = cost;
                if let EditKind::Typing {
                    segment,
                    closes_after,
                } = kind
                {
                    previous.typing = Some(TypingGroup {
                        segment,
                        last_ms: now_ms,
                        closed_after: closes_after,
                    });
                }
                self.undo.push_back(previous);
            }
            None => {
                let typing = match kind {
                    EditKind::Typing {
                        segment,
                        closes_after,
                    } => Some(TypingGroup {
                        segment,
                        last_ms: now_ms,
                        closed_after: closes_after,
                    }),
                    EditKind::Other => None,
                };
                self.undo.push_back(Entry {
                    transaction: inverse,
                    before,
                    after,
                    cost_bound,
                    typing,
                });
            }
        }
        Ok(())
    }

    /// Applies the latest undo transaction atomically and moves its inverse to
    /// the redo stack. An empty stack is a no-op.
    pub fn undo<S>(&mut self, state: &mut S) -> Result<bool, HistoryError<O::Error>>
    where
        S: TransactionalState,
        O: Operation<S>,
    {
        let Some(entry) = self.undo.back() else {
            return Ok(false);
        };
        self.redo
            .try_reserve(1)
            .map_err(|_| HistoryError::StorageReservationFailed)?;
        let inverse =
            entry
                .transaction
                .apply(state)
                .map_err(|source| HistoryError::Transaction {
                    action: HistoryAction::Undo,
                    source,
                })?;
        let mut entry = self.undo.pop_back().expect("entry was present");
        self.current_selection = entry.before.clone();
        entry.transaction = inverse;
        entry.typing = None;
        self.redo.push_back(entry);
        // Undo may reveal an older typing entry beneath a non-typing action.
        // That revealed entry must not resume grouping across the navigation.
        if let Some(previous) = self.undo.back_mut() {
            previous.typing = None;
        }
        Ok(true)
    }

    /// Applies the latest redo transaction atomically and moves its inverse to
    /// the undo stack. An empty stack is a no-op.
    pub fn redo<S>(&mut self, state: &mut S) -> Result<bool, HistoryError<O::Error>>
    where
        S: TransactionalState,
        O: Operation<S>,
    {
        let Some(entry) = self.redo.back() else {
            return Ok(false);
        };
        self.undo
            .try_reserve(1)
            .map_err(|_| HistoryError::StorageReservationFailed)?;
        let inverse =
            entry
                .transaction
                .apply(state)
                .map_err(|source| HistoryError::Transaction {
                    action: HistoryAction::Redo,
                    source,
                })?;
        let mut entry = self.redo.pop_back().expect("entry was present");
        self.current_selection = entry.after.clone();
        entry.transaction = inverse;
        entry.typing = None;
        self.undo.push_back(entry);
        Ok(true)
    }

    /// Number of entries currently available to undo.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Number of entries currently available to redo.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
}

fn minimum_entry_cost<O, Selection>() -> usize {
    // Even zero-sized user operations and snapshots consume a history entry and
    // a deque slot. The caller's bound covers variable heap allocations; this
    // fixed floor prevents arbitrary entry counts under a zero-cost estimate.
    core::mem::size_of::<Entry<O, Selection>>().saturating_add(core::mem::size_of::<usize>())
}

#[cfg(test)]
mod tests {
    use super::{EditKind, History, HistoryAction, HistoryError, minimum_entry_cost};
    use crate::{Operation, Transaction, TransactionError, TransactionalState};

    #[derive(Debug, Eq, PartialEq)]
    struct State(i32);
    impl TransactionalState for State {
        type Checkpoint = i32;
        fn checkpoint(&self) -> i32 {
            self.0
        }
        fn restore(&mut self, checkpoint: i32) {
            self.0 = checkpoint;
        }
    }

    struct OrderedState(Vec<u32>);
    impl TransactionalState for OrderedState {
        type Checkpoint = Vec<u32>;
        fn checkpoint(&self) -> Vec<u32> {
            self.0.clone()
        }
        fn restore(&mut self, checkpoint: Vec<u32>) {
            self.0 = checkpoint;
        }
    }

    enum OrderedOp {
        Push(u32),
        Pop(u32),
    }
    impl Operation<OrderedState> for OrderedOp {
        type Error = &'static str;
        fn apply(&self, state: &mut OrderedState) -> Result<(), Self::Error> {
            match self {
                Self::Push(value) => state.0.push(*value),
                Self::Pop(expected) => {
                    if state.0.pop() != Some(*expected) {
                        return Err("operation order mismatch");
                    }
                }
            }
            Ok(())
        }
        fn invert(&self, state: &OrderedState) -> Result<Self, Self::Error> {
            match self {
                Self::Push(value) => Ok(Self::Pop(*value)),
                Self::Pop(value) if state.0.last() == Some(value) => Ok(Self::Push(*value)),
                Self::Pop(_) => Err("unexpected stack value"),
            }
        }
        fn encode(&self, _: &mut crate::Encoder) -> Result<(), crate::CodecError> {
            Ok(())
        }
        fn decode(_: &mut crate::Decoder<'_>) -> Result<Self, crate::CodecError> {
            unreachable!()
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    struct Op {
        delta: i32,
        fail_apply: bool,
        fail_invert: bool,
        fail_inverse_invert: bool,
        fail_inverse_apply: bool,
        fail_redo_apply: bool,
    }
    impl Operation<State> for Op {
        type Error = &'static str;
        fn apply(&self, state: &mut State) -> Result<(), Self::Error> {
            state.0 += self.delta;
            if self.fail_apply {
                Err("apply")
            } else {
                Ok(())
            }
        }
        fn invert(&self, _: &State) -> Result<Self, Self::Error> {
            if self.fail_invert {
                Err("invert")
            } else {
                Ok(Self {
                    delta: -self.delta,
                    fail_apply: self.fail_inverse_apply,
                    fail_invert: self.fail_inverse_invert,
                    fail_inverse_invert: false,
                    fail_inverse_apply: self.fail_redo_apply,
                    fail_redo_apply: false,
                })
            }
        }
        fn encode(&self, _: &mut crate::Encoder) -> Result<(), crate::CodecError> {
            Ok(())
        }
        fn decode(_: &mut crate::Decoder<'_>) -> Result<Self, crate::CodecError> {
            unreachable!()
        }
    }

    fn tx(delta: i32) -> Transaction<Op> {
        Transaction::new(vec![Op {
            delta,
            fail_apply: false,
            fail_invert: false,
            fail_inverse_invert: false,
            fail_inverse_apply: false,
            fail_redo_apply: false,
        }])
        .unwrap()
    }

    #[test]
    fn apply_undo_redo_restore_state_and_selection() {
        let mut state = State(0);
        let mut history = History::new((crate::NodeId(1), 0usize), 100);
        history
            .apply(
                &mut state,
                tx(7),
                (crate::NodeId(1), 7),
                10,
                EditKind::Other,
                20,
            )
            .unwrap();
        assert_eq!(state.0, 7);
        assert_eq!(history.selection(), &(crate::NodeId(1), 7));
        assert!(history.undo(&mut state).unwrap());
        assert_eq!(state.0, 0);
        assert_eq!(history.selection(), &(crate::NodeId(1), 0));
        assert!(history.redo(&mut state).unwrap());
        assert_eq!(state.0, 7);
        assert_eq!(history.selection(), &(crate::NodeId(1), 7));
    }

    #[test]
    fn typing_group_boundaries_include_exactly_one_second_and_close_on_space_or_move() {
        let mut state = State(0);
        let mut history = History::new(0usize, 1_000);
        let typing = |segment| EditKind::Typing {
            segment,
            closes_after: false,
        };
        history
            .apply(&mut state, tx(1), 1, 100, typing(4), 10)
            .unwrap();
        history
            .apply(&mut state, tx(1), 2, 1_100, typing(4), 10)
            .unwrap();
        assert_eq!(history.undo_len(), 1, "1,000 ms remains in the group");
        history
            .apply(&mut state, tx(1), 3, 2_101, typing(4), 10)
            .unwrap();
        assert_eq!(history.undo_len(), 2, "1,001 ms starts a new group");
        history
            .apply(
                &mut state,
                tx(1),
                4,
                2_200,
                EditKind::Typing {
                    segment: 4,
                    closes_after: true,
                },
                10,
            )
            .unwrap();
        history
            .apply(&mut state, tx(1), 5, 2_201, typing(4), 10)
            .unwrap();
        assert_eq!(
            history.undo_len(),
            3,
            "space-after-word closes after itself"
        );
        history.move_cursor(99);
        history
            .apply(&mut state, tx(1), 6, 2_202, typing(4), 10)
            .unwrap();
        assert_eq!(history.undo_len(), 4, "cursor movement closes typing");
    }

    #[test]
    fn budget_rejects_oversized_and_evicts_oldest_only_after_success() {
        let mut state = State(0);
        let floor = minimum_entry_cost::<Op, usize>();
        let mut history = History::new(0usize, floor + 20);
        assert!(matches!(
            history.apply(&mut state, tx(1), 1, 0, EditKind::Other, floor + 21),
            Err(HistoryError::BudgetExceeded { .. })
        ));
        assert_eq!(state.0, 0);
        assert_eq!(history.undo_len(), 0);
        history
            .apply(&mut state, tx(1), 1, 1, EditKind::Other, 10)
            .unwrap();
        history
            .apply(&mut state, tx(2), 2, 2, EditKind::Other, floor + 15)
            .unwrap();
        assert_eq!(history.undo_len(), 1);
        assert!(history.retained_cost_bound().unwrap() >= floor + 15);
        let mut zero = History::<Op, usize>::new(0, 0);
        assert!(matches!(
            zero.apply(&mut state, tx(1), 1, 0, EditKind::Other, 0),
            Err(HistoryError::BudgetExceeded { .. })
        ));
        assert_eq!(state.0, 3);
    }

    #[test]
    fn over_budget_typing_group_splits_and_respects_the_budget() {
        let mut state = State(0);
        let floor = minimum_entry_cost::<Op, usize>();
        let per_entry = floor + 1;
        let mut history = History::new(0usize, floor * 2 + 1);
        let typing = EditKind::Typing {
            segment: 7,
            closes_after: false,
        };
        history
            .apply(&mut state, tx(1), 1, 0, typing, per_entry)
            .unwrap();
        history
            .apply(&mut state, tx(1), 2, 1, typing, per_entry)
            .unwrap();
        assert_eq!(history.undo_len(), 1);
        assert_eq!(history.retained_cost_bound().unwrap(), per_entry);
        assert!(history.retained_cost_bound().unwrap() <= history.budget_bytes());
    }

    #[test]
    fn empty_edit_and_failed_edit_preserve_redo_and_selection() {
        let mut state = State(0);
        let mut history = History::new(0usize, minimum_entry_cost::<Op, usize>() + 100);
        history
            .apply(&mut state, tx(3), 3, 1, EditKind::Other, 10)
            .unwrap();
        history.undo(&mut state).unwrap();
        let empty = Transaction::new(Vec::new()).unwrap();
        history
            .apply(&mut state, empty, 99, 2, EditKind::Other, 0)
            .unwrap();
        assert_eq!(history.redo_len(), 1);
        assert_eq!(history.selection(), &0);
        let failed = Transaction::new(vec![Op {
            delta: 100,
            fail_apply: true,
            fail_invert: false,
            fail_inverse_invert: false,
            fail_inverse_apply: false,
            fail_redo_apply: false,
        }])
        .unwrap();
        let error = history
            .apply(&mut state, failed, 99, 3, EditKind::Other, 10)
            .unwrap_err();
        assert!(matches!(
            error,
            HistoryError::Transaction {
                action: HistoryAction::Apply,
                source: TransactionError::Application {
                    index: 0,
                    source: "apply"
                }
            }
        ));
        assert_eq!(state.0, 0);
        assert_eq!(history.redo_len(), 1);
        assert_eq!(history.selection(), &0);
        history
            .apply(&mut state, tx(4), 4, 4, EditKind::Other, 10)
            .unwrap();
        assert_eq!(history.redo_len(), 0);
    }

    #[test]
    fn undo_and_redo_errors_preserve_stack_entry_and_selection() {
        let mut state = State(0);
        let mut history = History::new(0usize, minimum_entry_cost::<Op, usize>() + 100);
        let fail_undo = Transaction::new(vec![Op {
            delta: 2,
            fail_apply: false,
            fail_invert: false,
            fail_inverse_invert: true,
            fail_inverse_apply: false,
            fail_redo_apply: false,
        }])
        .unwrap();
        history
            .apply(&mut state, fail_undo, 2, 0, EditKind::Other, 10)
            .unwrap();
        let undo_error = history.undo(&mut state).unwrap_err();
        assert!(matches!(
            undo_error,
            HistoryError::Transaction {
                action: HistoryAction::Undo,
                source: TransactionError::Inversion {
                    index: 0,
                    source: "invert"
                }
            }
        ));
        assert_eq!(state.0, 2);
        assert_eq!(history.undo_len(), 1);
        assert_eq!(history.selection(), &2);
    }

    #[test]
    fn grouped_undo_and_redo_failures_restore_state_and_preserve_history() {
        let mut state = State(0);
        let floor = minimum_entry_cost::<Op, usize>();
        let mut history = History::new(0usize, floor * 4 + 100);
        history
            .apply(
                &mut state,
                Transaction::new(vec![Op {
                    delta: 1,
                    fail_apply: false,
                    fail_invert: false,
                    fail_inverse_invert: false,
                    fail_inverse_apply: true,
                    fail_redo_apply: false,
                }])
                .unwrap(),
                1,
                0,
                EditKind::Typing {
                    segment: 1,
                    closes_after: false,
                },
                10,
            )
            .unwrap();
        history
            .apply(
                &mut state,
                tx(2),
                2,
                1,
                EditKind::Typing {
                    segment: 1,
                    closes_after: false,
                },
                10,
            )
            .unwrap();
        assert_eq!(history.undo_len(), 1);
        assert_eq!(state.0, 3);
        let error = history.undo(&mut state).unwrap_err();
        assert!(matches!(
            error,
            HistoryError::Transaction {
                action: HistoryAction::Undo,
                source: TransactionError::Application {
                    index: 1,
                    source: "apply"
                }
            }
        ));
        assert_eq!(state.0, 3, "first grouped inverse mutation was rolled back");
        assert_eq!(history.undo_len(), 1);
        assert_eq!(history.redo_len(), 0);
        assert_eq!(history.selection(), &2);

        let mut redo_history = History::new(0usize, floor * 4 + 100);
        redo_history
            .apply(
                &mut state,
                Transaction::new(vec![
                    Op {
                        delta: 1,
                        fail_apply: false,
                        fail_invert: false,
                        fail_inverse_invert: false,
                        fail_inverse_apply: false,
                        fail_redo_apply: false,
                    },
                    Op {
                        delta: 2,
                        fail_apply: false,
                        fail_invert: false,
                        fail_inverse_invert: false,
                        fail_inverse_apply: false,
                        fail_redo_apply: true,
                    },
                ])
                .unwrap(),
                2,
                0,
                EditKind::Other,
                10,
            )
            .unwrap();
        redo_history.undo(&mut state).unwrap();
        assert_eq!(redo_history.selection(), &0);
        let before_redo = state.0;
        let error = redo_history.redo(&mut state).unwrap_err();
        assert!(matches!(
            error,
            HistoryError::Transaction {
                action: HistoryAction::Redo,
                source: TransactionError::Application {
                    index: 1,
                    source: "apply"
                }
            }
        ));
        assert_eq!(state.0, before_redo, "partial redo was rolled back");
        assert_eq!(redo_history.undo_len(), 0);
        assert_eq!(redo_history.redo_len(), 1);
        assert_eq!(redo_history.selection(), &0);
    }

    #[test]
    fn typing_group_starts_a_new_entry_at_the_transaction_operation_limit() {
        let mut state = OrderedState(Vec::new());
        let floor = minimum_entry_cost::<OrderedOp, usize>();
        let full_cost =
            floor + crate::MAX_TRANSACTION_OPERATIONS * core::mem::size_of::<OrderedOp>() * 2;
        let next_cost = floor + core::mem::size_of::<OrderedOp>() * 2;
        let mut history = History::new(0usize, full_cost + next_cost);
        let full = Transaction::new(
            (0..crate::MAX_TRANSACTION_OPERATIONS as u32)
                .map(OrderedOp::Push)
                .collect(),
        )
        .unwrap();
        history
            .apply(
                &mut state,
                full,
                crate::MAX_TRANSACTION_OPERATIONS,
                0,
                EditKind::Typing {
                    segment: 1,
                    closes_after: false,
                },
                full_cost,
            )
            .unwrap();
        history
            .apply(
                &mut state,
                Transaction::new(vec![OrderedOp::Push(
                    crate::MAX_TRANSACTION_OPERATIONS as u32,
                )])
                .unwrap(),
                crate::MAX_TRANSACTION_OPERATIONS + 1,
                1,
                EditKind::Typing {
                    segment: 1,
                    closes_after: false,
                },
                next_cost,
            )
            .unwrap();
        assert_eq!(history.undo_len(), 2);
        assert_eq!(
            state.0,
            (0..=crate::MAX_TRANSACTION_OPERATIONS as u32 - 1)
                .collect::<Vec<_>>()
                .into_iter()
                .chain([crate::MAX_TRANSACTION_OPERATIONS as u32])
                .collect::<Vec<_>>()
        );
        assert!(history.undo(&mut state).unwrap());
        assert_eq!(
            state.0,
            (0..crate::MAX_TRANSACTION_OPERATIONS as u32).collect::<Vec<_>>()
        );
        assert_eq!(*history.selection(), crate::MAX_TRANSACTION_OPERATIONS);
        assert!(history.undo(&mut state).unwrap());
        assert!(state.0.is_empty());
        assert_eq!(*history.selection(), 0);
        assert!(history.redo(&mut state).unwrap());
        assert_eq!(
            state.0,
            (0..crate::MAX_TRANSACTION_OPERATIONS as u32).collect::<Vec<_>>()
        );
        assert_eq!(*history.selection(), crate::MAX_TRANSACTION_OPERATIONS);
        assert!(history.redo(&mut state).unwrap());
        assert_eq!(
            state.0,
            (0..=crate::MAX_TRANSACTION_OPERATIONS as u32).collect::<Vec<_>>()
        );
        assert_eq!(*history.selection(), crate::MAX_TRANSACTION_OPERATIONS + 1);
    }

    #[test]
    fn undoing_non_typing_edit_closes_the_exposed_typing_group() {
        let mut state = State(0);
        let mut history = History::new(0usize, minimum_entry_cost::<Op, usize>() * 10);
        let typing = EditKind::Typing {
            segment: 9,
            closes_after: false,
        };
        history.apply(&mut state, tx(1), 1, 10, typing, 10).unwrap();
        history
            .apply(&mut state, tx(10), 11, 20, EditKind::Other, 10)
            .unwrap();
        assert!(history.undo(&mut state).unwrap());
        assert_eq!(state.0, 1);
        assert_eq!(*history.selection(), 1);
        history.apply(&mut state, tx(2), 3, 30, typing, 10).unwrap();
        assert_eq!(
            history.undo_len(),
            2,
            "C is separate from A after undoing B"
        );
        assert!(history.undo(&mut state).unwrap());
        assert_eq!(state.0, 1, "undoing C leaves A applied");
        assert_eq!(
            *history.selection(),
            1,
            "A's after-selection remains intact"
        );
        assert_eq!(history.undo_len(), 1);
        assert!(history.undo(&mut state).unwrap());
        assert_eq!(state.0, 0);
        assert_eq!(*history.selection(), 0);
    }
}
