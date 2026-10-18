// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-113 : diagnostic en ligne de commande du pont natif (`--bridge-self-check <dossier sous target/>`).
//! Il charge la vraie page dans une WebView2 masquée et exerce, sans dialogue interactif, les
//! tickets, la lecture par SharedBuffer d'un fichier de 100 Mio (médiane de plusieurs essais),
//! l'écriture atomique par morceaux, la liste et la lecture de polices. Les tickets sont remis par le
//! code natif de confiance (comme le ferait un dialogue après le choix de l'utilisateur) ; le script
//! n'a accès qu'à l'API publique du pont.

use super::*;
use crate::bridge_ops::{self, TICKET_READ_LIMIT};
use std::sync::atomic::Ordering as AtomicOrdering;

const BIG_BYTES: usize = 100 * 1024 * 1024;
const TRIALS: usize = 7;
/// Cible de la story : 100 Mo lus et reçus par le JS en moins de 300 ms.
const TARGET_MS: f64 = 300.0;
const WRITE_BYTES: usize = 300_000;

const START: &str = r#"(()=>{
  if(location.origin!=='https://app.atelier')return false;
  if(window.__atelierUs113Started)return true;
  window.__atelierUs113Started=true;
  window.__atelierUs113Result=null;
  const cfg=__CONFIG__;
  void (async()=>{
    try{
      const ready=await (await import('/app.mjs')).editorReady;
      if(ready.status!=='ready'||!ready.host)throw new Error('EDITOR_NOT_READY');
      const host=ready.host;
      const pat=i=>((((i&0xfffff)*7)+3)&255);
      const times=[];
      for(const ticket of cfg.trials){
        const t0=performance.now();
        const r=await host.readTicket(ticket);
        const dt=performance.now()-t0;
        if(r.kind!=='ok')throw new Error('READ:'+JSON.stringify(r));
        if(!r.shared||r.bytes.byteLength!==cfg.big)throw new Error('READ_SHAPE:'+r.shared+':'+r.bytes.byteLength);
        for(const i of [0,1,255,256,1048575,1048576,cfg.big>>1,cfg.big-1])if(r.bytes[i]!==pat(i))throw new Error('READ_DATA:'+i);
        r.release();
        times.push(dt);
      }
      const sorted=[...times].sort((a,b)=>a-b);
      const median=sorted[sorted.length>>1];
      const small=await host.readTicket(cfg.small);
      const smallOk=small.kind==='ok'&&!small.shared&&new TextDecoder().decode(small.bytes)===cfg.smallText;
      const payload=new Uint8Array(cfg.writeBytes);
      for(let i=0;i<payload.length;i++)payload[i]=(i*31+5)&255;
      const written=await host.writeTicket(cfg.write,payload);
      const listed=await host.listFonts();
      if(listed.kind!=='ok'||!listed.fonts.length)throw new Error('FONTS:'+JSON.stringify(listed));
      const candidate=listed.fonts.find(f=>f.size>65536&&f.size<5000000)||listed.fonts[0];
      const font=await host.readFont(candidate.id);
      const magic=font.kind==='ok'?[...font.bytes.slice(0,4)].map(b=>b.toString(16).padStart(2,'0')).join(''):'';
      const fontOk=font.kind==='ok'&&font.bytes.byteLength===candidate.size&&['00010000','4f54544f','74727565','74746366'].includes(magic);
      const fontShared=font.kind==='ok'&&font.shared===true;
      if(font.kind==='ok')font.release();
      const bad=await host.readFont('f1-99999');
      const again=await host.readTicket(cfg.trials[0]);
      const all=['txt','docx','docm','dotx','odt','doc'];
      const filters=all.map(format=>({format,label:'Format '+format}));
      const dialogOpen=await host.pickOpen({title:'Ouvrir (diagnostic)',filters});
      const dialogSave=await host.pickSave({title:'Enregistrer (diagnostic)',filters,suggestedName:'Document.docx'});
      const stats=host.stats();
      window.__atelierUs113Result={
        ok:true,times,median,min:sorted[0],max:sorted[sorted.length-1],smallOk,
        writeOk:written.kind==='ok',fonts:listed.fonts.length,fontOk,fontShared,
        badKey:bad.key||'',spentKey:again.key||'',dialogOpen:dialogOpen.kind,dialogSave:dialogSave.kind,stats
      };
    }catch(error){window.__atelierUs113Result={ok:false,error:String(error&&error.message||error)};}
  })();
  return true;
})()"#;
const POLL: &str = r#"({done:!!window.__atelierUs113Result,result:window.__atelierUs113Result})"#;

/// Valeur numérique d'une clé d'un objet JSON plat renvoyé par le script (sans analyseur JSON).
fn json_number(payload: &str, key: &str) -> Option<f64> {
    let marker = format!("\"{key}\":");
    let start = payload.find(&marker)? + marker.len();
    let end = payload[start..]
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E' | '+')))
        .map_or(payload.len(), |offset| start + offset);
    payload[start..end].parse().ok()
}

fn pattern_block() -> Vec<u8> {
    (0..1usize << 20)
        .map(|i| (((i & 0xf_ffff) * 7 + 3) & 255) as u8)
        .collect()
}

fn prepare_big_file(path: &Path) -> io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() == BIG_BYTES as u64) {
        return Ok(());
    }
    use std::io::Write;
    let block = pattern_block();
    let mut file = std::fs::File::create(path)?;
    for _ in 0..BIG_BYTES / block.len() {
        file.write_all(&block)?;
    }
    file.sync_all()
}

/// Point d'entrée : voir le module. `cache` doit se résoudre sous `target/` du dépôt.
pub fn bridge_self_check(state_file: &Path, cache: &Path) -> io::Result<String> {
    if probe_runtime_unavailable()? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Microsoft Edge WebView2 Evergreen runtime is unavailable",
        ));
    }
    let state_file = validate_self_check_path(state_file)?;
    let cache_marker = validate_self_check_path(&cache.join(".self-check-path"))?;
    let cache = cache_marker
        .parent()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "bridge self-check needs a directory",
            )
        })?
        .to_path_buf();
    if let Some(parent) = state_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&cache)?;
    let _apartment = webview::ComApartment::initialize_sta()?;
    let callback_baseline = WebView::live_callback_objects();
    let report = run_bridge_cycle(&state_file, &cache)?;
    let callbacks_after = WebView::live_callback_objects();
    if callbacks_after != callback_baseline {
        return Err(io::Error::other(format!(
            "WebView2 callback objects leaked during the bridge self-check: {callback_baseline}->{callbacks_after}"
        )));
    }
    let _ = std::fs::remove_file(&state_file);
    Ok(format!(
        "{report}; callback objects {callback_baseline}->{callbacks_after}"
    ))
}

fn run_bridge_cycle(state_file: &Path, cache: &Path) -> io::Result<String> {
    use std::time::{Duration, Instant};

    let files = cache.join("bridge-files");
    std::fs::create_dir_all(&files)?;
    let big = files.join("bridge-100mib.bin");
    prepare_big_file(&big)?;
    let small = files.join("bridge-small.txt");
    let small_text = "petit fichier de contrôle é€";
    std::fs::write(&small, small_text)?;
    let write_target = files.join("bridge-write.bin");
    let _ = std::fs::remove_file(&write_target);

    let mut runtime = FileRuntime::new().map_err(|error| io::Error::other(error.message_key))?;
    let issue = |error: crate::files::FileError| io::Error::other(error.message_key);
    let mut trials = Vec::new();
    for _ in 0..TRIALS {
        trials.push(
            runtime
                .bridge
                .issue_read_ticket(&runtime.broker, &big, TICKET_READ_LIMIT)
                .map_err(issue)?,
        );
    }
    let small_ticket = runtime
        .bridge
        .issue_read_ticket(&runtime.broker, &small, TICKET_READ_LIMIT)
        .map_err(issue)?;
    let write_ticket = runtime
        .bridge
        .issue_write_ticket(&runtime.broker, &write_target)
        .map_err(issue)?;
    let config = format!(
        "{{trials:[{}],small:'{small_ticket}',smallText:'{small_text}',write:'{write_ticket}',big:{BIG_BYTES},writeBytes:{WRITE_BYTES}}}",
        trials
            .iter()
            .map(|ticket| format!("'{ticket}'"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let start_script = START.replace("__CONFIG__", &config);

    let posted0 = bridge_ops::SHARED_BUFFERS_POSTED.load(AtomicOrdering::Relaxed);
    let unavailable0 = bridge_ops::SHARED_BUFFER_UNAVAILABLE.load(AtomicOrdering::Relaxed);
    let failed0 = bridge_ops::SHARED_BUFFER_FAILED.load(AtomicOrdering::Relaxed);

    let saved = load_state(state_file)?;
    let mut context = create_context(state_file, "Atelier bridge self-check", saved)?;
    context.file_runtime = Some(runtime);
    let initial = restore_state(saved, &monitors()?, context.dpi);
    let window = create_window(&mut context, initial)?;
    let hwnd = window.hwnd;
    // SAFETY: hwnd is a live hidden window; use the actual DPI context of its current monitor.
    context.dpi = unsafe { GetDpiForWindow(hwnd) }.clamp(48, 960);
    context.webview = Some(WebView::begin(hwnd, cache)?);
    // SAFETY: hwnd is the hidden diagnostic window owned by this routine.
    if unsafe { SetTimer(hwnd, TIMER_WEBVIEW_INIT, webview::timeout_ms(), null_mut()) } == 0 {
        return Err(error_from_last("SetTimer(bridge self-check)"));
    }
    pump_until_webview_ready(hwnd, &mut context)?;
    // Les dialogues communs ouverts par le diagnostic sont fermés (annulés) par cette minuterie.
    // SAFETY: hwnd est la fenêtre masquée du diagnostic.
    if unsafe { SetTimer(hwnd, TIMER_DIAG_DIALOG_CLOSE, 150, null_mut()) } == 0 {
        return Err(error_from_last("SetTimer(bridge dialog close)"));
    }

    let start_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let started = run_script_until(hwnd, &mut context, &start_script, Duration::from_secs(2))?;
        if started.trim() == "true" {
            break;
        }
        if Instant::now() >= start_deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("embedded origin did not become active; ExecuteScript returned {started}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let deadline = Instant::now() + Duration::from_secs(180);
    let payload = loop {
        let value = run_script_until(hwnd, &mut context, POLL, Duration::from_secs(5))?;
        if value.contains("\"done\":true") {
            break value;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("bridge self-check timed out; last result={value}"),
            ));
        }
    };
    let posted = bridge_ops::SHARED_BUFFERS_POSTED.load(AtomicOrdering::Relaxed) - posted0;
    let unavailable =
        bridge_ops::SHARED_BUFFER_UNAVAILABLE.load(AtomicOrdering::Relaxed) - unavailable0;
    let failed = bridge_ops::SHARED_BUFFER_FAILED.load(AtomicOrdering::Relaxed) - failed0;
    if unavailable > 0 {
        return Err(io::Error::other(format!(
            "SharedBuffer interfaces are unavailable in this WebView2 runtime ({unavailable} base64 fallbacks); payload={payload}"
        )));
    }
    for needle in [
        "\"ok\":true",
        "\"smallOk\":true",
        "\"writeOk\":true",
        "\"fontOk\":true",
        "\"badKey\":\"fonts.error.unknown_id\"",
        "\"spentKey\":\"files.error.unknown_ticket\"",
        "\"held\":0",
        "\"dialogOpen\":\"cancelled\"",
        "\"dialogSave\":\"cancelled\"",
    ] {
        if !payload.contains(needle) {
            return Err(io::Error::other(format!(
                "bridge self-check failed ({needle} missing; shared posted={posted} failed={failed}): {payload}"
            )));
        }
    }
    if failed > 0 {
        return Err(io::Error::other(format!(
            "SharedBuffer posting failed {failed} times: {:?}",
            bridge_ops::LAST_SHARED_BUFFER_ERROR
                .lock()
                .ok()
                .and_then(|e| e.clone())
        )));
    }
    let written = std::fs::read(&write_target)?;
    let expected: Vec<u8> = (0..WRITE_BYTES)
        .map(|i| ((i * 31 + 5) & 255) as u8)
        .collect();
    if written != expected {
        return Err(io::Error::other(
            "the atomic write through the bridge produced different bytes",
        ));
    }
    let (median, min, max) = (
        json_number(&payload, "median"),
        json_number(&payload, "min"),
        json_number(&payload, "max"),
    );
    let (Some(median), Some(min), Some(max)) = (median, min, max) else {
        return Err(io::Error::other(format!("no timing in {payload}")));
    };
    let released = json_number(&payload, "sharedReleased").unwrap_or(-1.0);
    let received = json_number(&payload, "sharedReceived").unwrap_or(-2.0);
    if released != received {
        return Err(io::Error::other(format!(
            "shared buffers received {received} but released {released}"
        )));
    }
    let version = webview::probe_runtime()?.unwrap_or_default();
    let verdict = if median < TARGET_MS { "MET" } else { "NOT MET" };

    // SAFETY: même fenêtre ; la minuterie n'est plus utile.
    unsafe { KillTimer(hwnd, TIMER_DIAG_DIALOG_CLOSE) };
    {
        let webview = context.webview.as_mut().expect("created WebView2");
        webview.set_visible(false)?;
        webview.close_for_check()?;
    }
    pump_after_webview_close(hwnd, &mut context)?;
    context
        .webview
        .as_mut()
        .expect("created WebView2")
        .detach_window();
    // SAFETY: this routine owns hwnd and destroys it before context is dropped.
    if unsafe { DestroyWindow(hwnd) } == 0 {
        return Err(error_from_last("DestroyWindow(bridge self-check)"));
    }
    drop(window);
    drop(context);
    let mut quit = Msg {
        hwnd: null_mut(),
        message: 0,
        w_param: 0,
        l_param: 0,
        time: 0,
        point: Point::default(),
        private: 0,
    };
    // SAFETY: quit is writable; PM_REMOVE consumes only WM_QUIT messages left by this cycle.
    while unsafe { PeekMessageW(&mut quit, null_mut(), 0x0012, 0x0012, 0x0001) } != 0 {}
    let _ = std::fs::remove_file(&write_target);
    Ok(format!(
        "bridge: shared-buffer read of {} MiB x{TRIALS}: median={median:.1} ms min={min:.1} ms max={max:.1} ms (target < {TARGET_MS} ms: {verdict}); \
         buffers posted={posted} received={received} released={released}; real common open/save dialogs shown with 6 format filters and cancelled, small inline read, {WRITE_BYTES}-byte chunked atomic write, \
         fonts={} (font read {}), spent/unknown tickets refused; WebView2 runtime {version}",
        BIG_BYTES >> 20,
        json_number(&payload, "fonts").unwrap_or(0.0),
        if payload.contains("\"fontShared\":true") {
            "via shared buffer"
        } else {
            "inline"
        },
    ))
}
