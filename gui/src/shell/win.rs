//! Запись плана [`super::registry`] в реестр через WinAPI.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteKeyW, RegDeleteTreeW, RegGetValueW,
    RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

use super::registry::{self, CLASSES, EXT, PROG_ID};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn full(key: &str) -> String {
    format!(r"{CLASSES}\{key}")
}

fn err(what: &str, key: &str, code: u32) -> String {
    format!("{what} HKCU\\{} — {}", full(key), std::io::Error::from_raw_os_error(code as i32))
}

/// Записывает все значения плана и сообщает Проводнику об изменениях.
pub fn register(exe: &Path) -> Result<(), String> {
    for value in registry::plan(exe) {
        set_string(&value.key, value.name, &value.data)?;
    }
    notify_shell();
    Ok(())
}

/// Удаляет наши ключи; ассоциацию `.plz` снимает, только если она наша.
pub fn unregister() -> Result<(), String> {
    for key in registry::owned_keys() {
        delete_tree(&key)?;
    }
    delete_value(&format!(r"{EXT}\OpenWithProgids"), Some(PROG_ID))?;
    if get_string(EXT, None).as_deref() == Some(PROG_ID) {
        delete_value(EXT, None)?;
    }
    notify_shell();
    Ok(())
}

/// Команда «Открыть», если PlicZip зарегистрирован.
pub fn registered_command() -> Option<String> {
    get_string(&format!(r"{PROG_ID}\shell\open\command"), None)
}

fn set_string(key: &str, name: Option<&str>, data: &str) -> Result<(), String> {
    let path = wide(&full(key));
    let mut hkey: HKEY = null_mut();
    // SAFETY: строки завершены нулём и живут до конца вызова; hkey закрывается ниже.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut hkey,
            null_mut(),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(err("не удалось создать ключ", key, code));
    }
    let name_w = name.map(wide);
    let data_w = wide(data);
    // SAFETY: data_w — UTF-16 с завершающим нулём, размер передан в байтах.
    let code = unsafe {
        RegSetValueExW(
            hkey,
            name_w.as_ref().map_or(null(), |n| n.as_ptr()),
            0,
            REG_SZ,
            data_w.as_ptr().cast(),
            (data_w.len() * 2) as u32,
        )
    };
    // SAFETY: hkey открыт успешным RegCreateKeyExW.
    unsafe { RegCloseKey(hkey) };
    if code != ERROR_SUCCESS {
        return Err(err("не удалось записать значение в", key, code));
    }
    Ok(())
}

fn get_string(key: &str, name: Option<&str>) -> Option<String> {
    let path = wide(&full(key));
    let name_w = name.map(wide);
    let name_ptr = name_w.as_ref().map_or(null(), |n| n.as_ptr());
    let mut bytes: u32 = 0;
    // SAFETY: первый вызов только узнаёт размер.
    let code = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), name_ptr, RRF_RT_REG_SZ, null_mut(), null_mut(), &mut bytes)
    };
    if code != ERROR_SUCCESS || bytes == 0 {
        return None;
    }
    let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
    // SAFETY: буфер не меньше запрошенного размера.
    let code = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            name_ptr,
            RRF_RT_REG_SZ,
            null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut bytes,
        )
    };
    if code != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn delete_tree(key: &str) -> Result<(), String> {
    let path = full(key);
    let path_w = wide(&path);
    // SAFETY: строка завершена нулём.
    let code = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, path_w.as_ptr()) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if code != ERROR_SUCCESS {
        return Err(err("не удалось удалить", key, code));
    }
    // На случай, если RegDeleteTreeW очистил ключ, но оставил его самого.
    let (parent, leaf) = path.rsplit_once('\\').unwrap_or(("", path.as_str()));
    let (parent_w, leaf_w) = (wide(parent), wide(leaf));
    let mut hkey: HKEY = null_mut();
    // SAFETY: строки завершены нулём; hkey закрывается ниже.
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, parent_w.as_ptr(), 0, KEY_ALL_ACCESS, &mut hkey) == ERROR_SUCCESS {
            RegDeleteKeyW(hkey, leaf_w.as_ptr());
            RegCloseKey(hkey);
        }
    }
    Ok(())
}

fn delete_value(key: &str, name: Option<&str>) -> Result<(), String> {
    let path = wide(&full(key));
    let name_w = name.map(wide);
    // SAFETY: строки завершены нулём.
    let code = unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, path.as_ptr(), name_w.as_ref().map_or(null(), |n| n.as_ptr()))
    };
    if code == ERROR_SUCCESS || code == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(err("не удалось удалить значение в", key, code))
    }
}

/// Просит Проводник перечитать ассоциации — иначе меню обновится после перезапуска.
fn notify_shell() {
    // SAFETY: для SHCNE_ASSOCCHANGED оба элемента должны быть null.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, null(), null()) };
}
