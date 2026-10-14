//! Offline native acceptance: production Project UI, no application setup or I/O.
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};
use usher_lib::{MainWindow, ProjectBridge, ProjectItem, Route};

type Rows = Rc<RefCell<Vec<ProjectItem>>>;

fn project(ui: &MainWindow, rows: &Rows) {
    let bridge = ui.global::<ProjectBridge>();
    let query = bridge.get_search_query().to_lowercase();
    bridge.set_items(ModelRc::new(VecModel::from(
        rows.borrow()
            .iter()
            .filter(|row| row.project_name.to_lowercase().contains(&query))
            .cloned()
            .collect::<Vec<_>>(),
    )));
}

fn main() -> Result<(), slint::PlatformError> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let locale = args
        .windows(2)
        .find(|pair| pair[0] == "--locale")
        .map_or("en", |pair| pair[1].as_str());
    let ui = MainWindow::new()?;
    slint::select_bundled_translation(locale).expect("use en or zh-CN");
    ui.window().set_size(LogicalSize::new(1_200.0, 900.0));
    ui.set_platform_is_macos(false);
    ui.set_show_navigation_drawer(false);
    ui.set_show_breadcrumb(false);
    ui.set_current_route(Route::Project);
    ui.global::<ProjectBridge>().set_hooks_ready(true);
    let view = args
        .windows(2)
        .find(|pair| pair[0] == "--view")
        .map_or("grid", |pair| pair[1].as_str());
    assert!(matches!(view, "grid" | "list"), "use --view grid or list");
    ui.global::<ProjectBridge>().set_view_mode(view.into());
    let rows: Rows = Rc::new(RefCell::new(vec![ProjectItem {
        project_name: "Offline Project / 离线项目".into(),
        project_alias: "offline-project".into(),
        local_repo: "/synthetic/project".into(),
        remote_repo: "https://example.test/synthetic/project.git".into(),
        repo_type: "github".into(),
        project_tags_text: "fixture, offline".into(),
        is_pr_auto_update: true,
        pr_fat_count_text: "2".into(),
        pr_uat_count_text: "1".into(),
        pr_prod_count_text: "0".into(),
        ..Default::default()
    }]));
    project(&ui, &rows);

    let (weak, data) = (ui.as_weak(), rows.clone());
    ui.global::<ProjectBridge>().on_search_projects(move |_| {
        if let Some(ui) = weak.upgrade() {
            project(&ui, &data);
        }
    });
    let (weak, data) = (ui.as_weak(), rows.clone());
    ui.global::<ProjectBridge>().on_reload_projects(move || {
        if let Some(ui) = weak.upgrade() {
            project(&ui, &data);
        }
    });
    let (weak, data) = (ui.as_weak(), rows.clone());
    ui.global::<ProjectBridge>().on_add_project(
        move |name, alias, local, remote, logo, tags, prefetch| {
            data.borrow_mut().push(ProjectItem {
                project_name: name,
                project_alias: alias,
                local_repo: local,
                remote_repo: remote,
                logo,
                project_tags_text: tags,
                repo_type: "github".into(),
                is_pr_auto_update: prefetch,
                ..Default::default()
            });
            if let Some(ui) = weak.upgrade() {
                project(&ui, &data);
            }
        },
    );
    let (weak, data) = (ui.as_weak(), rows.clone());
    ui.global::<ProjectBridge>().on_update_project(
        move |original, name, alias, local, remote, logo, tags, prefetch| {
            if let Some(row) = data
                .borrow_mut()
                .iter_mut()
                .find(|row| row.project_alias == original)
            {
                row.project_name = name;
                row.project_alias = alias;
                row.local_repo = local;
                row.remote_repo = remote;
                row.logo = logo;
                row.project_tags_text = tags;
                row.is_pr_auto_update = prefetch;
                eprintln!(
                    "Offline saved project {}: prefetch={prefetch}",
                    row.project_alias
                );
            }
            if let Some(ui) = weak.upgrade() {
                project(&ui, &data);
            }
        },
    );
    let (weak, data) = (ui.as_weak(), rows);
    ui.global::<ProjectBridge>()
        .on_delete_project(move |alias| {
            data.borrow_mut().retain(|row| row.project_alias != alias);
            if let Some(ui) = weak.upgrade() {
                project(&ui, &data);
            }
        });
    ui.run()
}
