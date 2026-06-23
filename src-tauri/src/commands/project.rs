use crate::config_store::{self, Project};
use crate::error::{AppError, Result};

#[tauri::command]
pub fn list_projects() -> Result<Vec<Project>> {
    let cfg = config_store::read()?;
    Ok(cfg.projects)
}

#[tauri::command]
pub fn add_project(name: String, path: String, identity_id: Option<String>) -> Result<Project> {
    let mut cfg = config_store::read()?;
    let id = config_store::new_id();
    let project = Project {
        id: id.clone(),
        name,
        path,
        identity_id,
    };
    cfg.projects.push(project.clone());
    config_store::write_snapshot(
        &cfg,
        "add_project",
        &format!("Added project {}", project.name),
    )?;
    Ok(project)
}

#[tauri::command]
pub fn remove_project(id: String) -> Result<()> {
    let mut cfg = config_store::read()?;
    let initial = cfg.projects.len();
    cfg.projects.retain(|p| p.id != id);
    if cfg.projects.len() == initial {
        return Err(AppError::NotFound(format!("project {}", id)));
    }
    config_store::write_snapshot(
        &cfg,
        "remove_project",
        &format!("Removed project {}", id),
    )?;
    Ok(())
}

#[tauri::command]
pub fn assign_identity(project_id: String, identity_id: String) -> Result<Project> {
    let mut cfg = config_store::read()?;
    let project = cfg
        .projects
        .iter_mut()
        .find(|p| p.id == project_id)
        .ok_or_else(|| AppError::NotFound(format!("project {}", project_id)))?;
    project.identity_id = Some(identity_id);
    let snapshot = project.clone();
    config_store::write_snapshot(
        &cfg,
        "assign_identity",
        &format!("Assigned identity to project {}", snapshot.name),
    )?;
    Ok(snapshot)
}
