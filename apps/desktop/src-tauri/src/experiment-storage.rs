use crate::cli::model::valid_id;
use crate::cli_policy::artifact_path_allowed;
use serde_json::Value;
use tauri::Manager;

pub(crate) fn parse_experiment(id: &str, data: &str) -> Result<Value, String> {
    if !valid_id(id) || data.len() > 8 * 1024 * 1024 {
        return Err("Invalid experiment".into());
    }
    serde_json::from_str(data).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn save_experiment(app: tauri::AppHandle, id: String, data: String) -> Result<String, String> {
    let value = parse_experiment(&id, &data)?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("experiments")
        .join(id);
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let temp = directory.join("result.tmp");
    std::fs::write(&temp, data).map_err(|e| e.to_string())?;
    std::fs::rename(temp, directory.join("result.json")).map_err(|e| e.to_string())?;
    if let Some(people) = value["people"].as_array() {
        for (index, person) in people.iter().take(2).enumerate() {
            let folder = directory.join(if index == 0 { "person-a" } else { "person-b" });
            if let Some(files) = person["artifacts"].as_object() {
                for (path, content) in files {
                    let valid = artifact_path_allowed(path);
                    if valid {
                        if let Some(content) = content.as_str() {
                            let target = folder.join(path);
                            std::fs::create_dir_all(
                                target.parent().ok_or("Invalid artifact path")?,
                            )
                            .map_err(|e| e.to_string())?;
                            std::fs::write(target, content).map_err(|e| e.to_string())?;
                        }
                    }
                }
            }
        }
    }
    Ok(directory.to_string_lossy().into_owned())
}
