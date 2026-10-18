// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-113 : transfert de données volumineuses vers le script par SharedBuffer WebView2.
//!
//! Liaisons COM manuelles de `ICoreWebView2Environment12::CreateSharedBuffer` (emplacement 24 de
//! la table virtuelle), `ICoreWebView2SharedBuffer` (Size 3, Buffer 4, OpenStream 5,
//! FileMappingHandle 6, Close 7) et `ICoreWebView2_17::PostSharedBufferToScript` (emplacement 116),
//! vérifiés par comptage des méthodes du SDK 1.0.x. Chaque interface obtenue passe par le registre
//! de références de test (`note_acquired` / `release_interface`). Le runtime qui ne fournit pas
//! ces interfaces est détecté par `QueryInterface` : l'appelant se replie alors, de façon
//! explicite, sur le chemin Base64.

use super::*;

const IID_ENVIRONMENT12: Guid = Guid {
    data1: 0xf503_db9b,
    data2: 0x739f,
    data3: 0x48dd,
    data4: [0xb1, 0x51, 0xfd, 0xfc, 0xf2, 0x53, 0xf5, 0x4e],
};
const IID_WEBVIEW17: Guid = Guid {
    data1: 0x702e_75d4,
    data2: 0xfd44,
    data3: 0x434d,
    data4: [0x9d, 0x70, 0x1a, 0x68, 0xa6, 0xb1, 0x19, 0x2a],
};
const SLOT_QUERY_INTERFACE: usize = 0;
const SLOT_CREATE_SHARED_BUFFER: usize = 24;
const SLOT_BUFFER_PTR: usize = 4;
const SLOT_BUFFER_CLOSE: usize = 7;
const SLOT_POST_SHARED_BUFFER: usize = 116;
/// `COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY` : le script ne peut pas écrire dans le tampon.
const ACCESS_READ_ONLY: i32 = 0;
/// Taille maximale d'un tampon partagé (borne de sûreté, la lecture de fichier est déjà bornée).
pub(crate) const MAX_SHARED_BUFFER_BYTES: usize = 1 << 30;

#[derive(Debug)]
pub(crate) enum SharedBufferError {
    /// Le runtime n'expose pas `ICoreWebView2Environment12` / `ICoreWebView2_17` (repli Base64).
    Unavailable,
    Failed(io::Error),
}

/// Référence COM possédée, relâchée à la destruction (aussi sur les chemins d'erreur).
struct Owned(*mut c_void);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: ce garde possède exactement une référence acquise sur cette STA ; `release_interface`
        // ignore le pointeur nul et alimente le registre de test.
        unsafe { release_interface(self.0) };
    }
}

/// # Safety
/// `interface` doit être un pointeur COM vivant ; en cas de succès l'appelant possède une référence.
unsafe fn query_interface(interface: *mut c_void, iid: &Guid) -> Result<Owned, i32> {
    let mut out = null_mut();
    // SAFETY: QueryInterface est l'emplacement 0 de toute interface COM ; `iid` et `out` sont valides.
    let hr = unsafe {
        call_vtable::<unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32>(
            interface,
            SLOT_QUERY_INTERFACE,
        )(interface, iid, &mut out)
    };
    if hr < 0 {
        return Err(hr);
    }
    if out.is_null() {
        return Err(E_POINTER);
    }
    note_acquired();
    Ok(Owned(out))
}

impl WebView {
    /// Copie `bytes` dans un nouveau SharedBuffer et le poste au script en lecture seule, avec
    /// `additional_json` (objet JSON ASCII) comme `additionalData` de l'événement `sharedbufferreceived`.
    /// Doit être appelée sur la STA du WebView. Toutes les références obtenues sont relâchées.
    pub(crate) fn post_shared_buffer(
        &self,
        bytes: &[u8],
        additional_json: &str,
    ) -> Result<(), SharedBufferError> {
        let failed =
            |message: &str| SharedBufferError::Failed(io::Error::other(message.to_owned()));
        if bytes.is_empty() || bytes.len() > MAX_SHARED_BUFFER_BYTES {
            return Err(failed("shared buffer size is out of range"));
        }
        let (Some(controller), Some(environment)) = (self.controller, self.environment.as_ref())
        else {
            return Err(failed("WebView2 is not ready"));
        };
        // SAFETY: `environment.interface` est une référence d'environnement possédée et vivante.
        let env12 = match unsafe {
            query_interface(environment.interface as *mut c_void, &IID_ENVIRONMENT12)
        } {
            Ok(owned) => owned,
            Err(hr) if hr == E_NOINTERFACE => return Err(SharedBufferError::Unavailable),
            Err(hr) => {
                return Err(SharedBufferError::Failed(hresult_error(
                    "QueryInterface(ICoreWebView2Environment12)",
                    hr,
                )));
            }
        };
        let mut raw_buffer = null_mut();
        // SAFETY: env12 est vivant ; l'emplacement 24 est CreateSharedBuffer(UINT64, **buffer).
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, u64, *mut *mut c_void) -> i32>(
                env12.0,
                SLOT_CREATE_SHARED_BUFFER,
            )(env12.0, bytes.len() as u64, &mut raw_buffer)
        };
        if hr < 0 {
            return Err(SharedBufferError::Failed(hresult_error(
                "ICoreWebView2Environment12::CreateSharedBuffer",
                hr,
            )));
        }
        if raw_buffer.is_null() {
            return Err(failed("CreateSharedBuffer returned no buffer"));
        }
        note_acquired();
        let buffer = Owned(raw_buffer);
        let mut memory: *mut u8 = null_mut();
        // SAFETY: buffer est vivant ; l'emplacement 4 est Buffer(BYTE**).
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut u8) -> i32>(
                buffer.0,
                SLOT_BUFFER_PTR,
            )(buffer.0, &mut memory)
        };
        check_hresult("ICoreWebView2SharedBuffer::get_Buffer", hr)
            .map_err(SharedBufferError::Failed)?;
        if memory.is_null() {
            return Err(failed("shared buffer has no memory"));
        }
        // SAFETY: le tampon vient d'être créé avec exactement `bytes.len()` octets accessibles en
        // écriture à `memory` ; il ne chevauche pas `bytes` (mémoire partagée neuve).
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), memory, bytes.len()) };
        let mut core = null_mut();
        // SAFETY: controller est une référence possédée ; l'emplacement 25 est get_CoreWebView2.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32>(
                controller as *mut c_void,
                25,
            )(controller as *mut c_void, &mut core)
        };
        check_hresult("ICoreWebView2Controller::get_CoreWebView2", hr)
            .map_err(SharedBufferError::Failed)?;
        if core.is_null() {
            return Err(failed("WebView2 core interface is null"));
        }
        note_acquired();
        let core = Owned(core);
        // SAFETY: core est une référence possédée et vivante.
        let core17 = match unsafe { query_interface(core.0, &IID_WEBVIEW17) } {
            Ok(owned) => owned,
            Err(hr) if hr == E_NOINTERFACE => return Err(SharedBufferError::Unavailable),
            Err(hr) => {
                return Err(SharedBufferError::Failed(hresult_error(
                    "QueryInterface(ICoreWebView2_17)",
                    hr,
                )));
            }
        };
        let json: Vec<u16> = additional_json.encode_utf16().chain(Some(0)).collect();
        // SAFETY: core17 et buffer sont vivants ; l'emplacement 116 est PostSharedBufferToScript(
        // buffer, access, LPCWSTR json) ; `json` est terminé par NUL pendant l'appel synchrone.
        let hr = unsafe {
            call_vtable::<unsafe extern "system" fn(*mut c_void, *mut c_void, i32, *const u16) -> i32>(
                core17.0,
                SLOT_POST_SHARED_BUFFER,
            )(core17.0, buffer.0, ACCESS_READ_ONLY, json.as_ptr())
        };
        // Le script détient sa propre vue : libère ici la mémoire côté natif sans attendre le Release.
        // SAFETY: l'emplacement 7 est Close() ; son résultat n'influence pas la livraison.
        unsafe {
            let _ = call_vtable::<unsafe extern "system" fn(*mut c_void) -> i32>(
                buffer.0,
                SLOT_BUFFER_CLOSE,
            )(buffer.0);
        }
        check_hresult("ICoreWebView2_17::PostSharedBufferToScript", hr)
            .map_err(SharedBufferError::Failed)
    }
}
