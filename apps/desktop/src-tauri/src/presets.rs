use std::{
    collections::VecDeque,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: u64 = 131_072;
#[derive(Default)]
pub struct PendingPresets(Mutex<VecDeque<Result<String, String>>>);

fn validate_preset(data: &str) -> Result<(), String> {
    if data.len() as u64 > MAX_BYTES {
        return Err("Пресет превышает 128 КиБ.".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(data).map_err(|_| "Неверный JSON пресета.")?;
    if value["format"] != "proteus-personas" || value["version"] != 1 {
        return Err("Неподдерживаемый формат или версия .proteus.".into());
    }
    let task = value["task"].as_str().ok_or("Неверная задача в пресете.")?;
    let people = value["people"].as_array().ok_or("Нет персон в пресете.")?;
    if task.chars().count() > 4000 || people.len() != 2 {
        return Err("Неверный размер пресета.".into());
    }
    for person in people {
        let label = person["label"].as_str().ok_or("Неверное имя персоны.")?;
        let context = person["context"]
            .as_str()
            .ok_or("Неверный контекст персоны.")?;
        let urls = person["public-urls"]
            .as_array()
            .ok_or("Неверные ссылки персоны.")?;
        if label.trim().is_empty()
            || label.chars().count() > 80
            || context.chars().count() > 4000
            || urls.len() > 5
            || (context.trim().is_empty() && urls.is_empty())
            || urls.iter().any(|url| !url.is_string())
        {
            return Err("Неверные поля персоны.".into());
        }
    }
    Ok(())
}
fn read_preset(path: &Path) -> Result<String, String> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("proteus"))
    {
        return Err("Выберите файл .proteus.".into());
    }
    let file = File::open(path).map_err(|_| "Не удалось открыть пресет.")?;
    let meta = file
        .metadata()
        .map_err(|_| "Не удалось прочитать пресет.")?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err("Пресет должен быть файлом до 128 КиБ.".into());
    }
    let mut data = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut data)
        .map_err(|_| "Не удалось прочитать UTF-8 пресет.")?;
    validate_preset(&data)?;
    Ok(data)
}

pub fn queue_files(app: &tauri::AppHandle, paths: impl IntoIterator<Item = PathBuf>) {
    for path in paths {
        let data = read_preset(&path);
        if let Ok(mut pending) = app.state::<PendingPresets>().0.lock() {
            if pending.len() < 16 {
                pending.push_back(data);
            }
        }
    }
    let _ = app.emit("preset-opened", ());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
pub fn argument_paths(args: impl IntoIterator<Item = String>, cwd: &Path) -> Vec<PathBuf> {
    args.into_iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .filter_map(|arg| {
            let path = if arg.starts_with("file:") {
                tauri::Url::parse(&arg).ok()?.to_file_path().ok()?
            } else if arg.contains("://") {
                return None;
            } else {
                PathBuf::from(arg)
            };
            Some(if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            })
        })
        .collect()
}
#[tauri::command]
pub fn take_pending_presets(
    state: tauri::State<'_, PendingPresets>,
) -> Result<Vec<Result<String, String>>, String> {
    Ok(state
        .0
        .lock()
        .map_err(|_| "Очередь пресетов недоступна.")?
        .drain(..)
        .collect())
}
#[tauri::command]
pub async fn import_preset(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Персоны PROTEUS", &["proteus"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        read_preset(&file.into_path().map_err(|_| "Нужен локальный файл.")?).map(Some)
    })
    .await
    .map_err(|error| error.to_string())?
}
#[tauri::command]
pub async fn export_preset(app: tauri::AppHandle, data: String) -> Result<Option<String>, String> {
    validate_preset(&data)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Персоны PROTEUS", &["proteus"])
            .set_file_name("personas.proteus")
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = file.into_path().map_err(|_| "Нужен локальный файл.")?;
        if path.extension().is_none() {
            path.set_extension("proteus");
        }
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("proteus"))
        {
            return Err("Сохраните файл с расширением .proteus.".into());
        }
        std::fs::write(&path, data).map_err(|_| "Не удалось сохранить пресет.")?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn fixture() -> String {
        r#"{"format":"proteus-personas","version":1,"task":"","people":[{"label":"Анна","context":"Планирование","public-urls":[]},{"label":"Борис","context":"Импровизация","public-urls":[]}]}"#.into()
    }
    #[test]
    fn installers_register_the_custom_preset_type() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let association = &config["bundle"]["fileAssociations"][0];
        assert!(include_str!("../file-associations/proteus.desktop").contains("Exec={{exec}} %U"));
        assert!(include_str!("../file-associations/proteus.xml").contains("*.proteus"));
        assert_eq!(association["ext"][0], "proteus");
        assert_eq!(
            association["exportedType"]["identifier"],
            "dev.proteus-int.personas"
        );
        assert!(
            config["bundle"]["linux"]["deb"]["files"]["/usr/share/mime/packages/proteus.xml"]
                .is_string()
        );
    }
    #[test]
    fn file_arguments_handle_spaces_file_urls_and_relative_paths() {
        let cwd = std::env::temp_dir();
        let absolute = cwd.join("two people.proteus");
        let url = tauri::Url::from_file_path(&absolute).unwrap().to_string();
        assert_eq!(
            argument_paths(
                vec![
                    "proteus".into(),
                    "--flag".into(),
                    "https://example.org/x".into(),
                    "relative.proteus".into(),
                    url
                ],
                &cwd
            ),
            vec![cwd.join("relative.proteus"), absolute]
        );
    }
    #[test]
    fn invalid_versions_and_person_fields_are_rejected() {
        for data in [
            fixture().replace("\"version\":1", "\"version\":2"),
            fixture().replace("\"context\":\"Планирование\"", "\"context\":12"),
            fixture().replace("\"public-urls\":[]", "\"public-urls\":[12]"),
        ] {
            assert!(validate_preset(&data).is_err());
        }
    }
    #[test]
    fn reads_unicode_preset_and_rejects_wrong_extension_invalid_and_large_files() {
        let root = std::env::temp_dir().join(format!("proteus-presets-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("персоны.proteus");
        fs::write(&path, fixture()).unwrap();
        assert_eq!(read_preset(&path).unwrap(), fixture());
        let other = root.join("other.json");
        fs::write(&other, fixture()).unwrap();
        assert!(read_preset(&other).is_err());
        fs::write(&path, "{}").unwrap();
        assert!(read_preset(&path).is_err());
        fs::write(&path, "x".repeat(MAX_BYTES as usize + 1)).unwrap();
        assert!(read_preset(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
