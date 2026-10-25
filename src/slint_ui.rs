use crate::tui::run_imposition_work;

type BusyReceiver = std::sync::mpsc::Receiver<(String, Option<std::path::PathBuf>)>;
use imposer::compute_layout;
use imposer::config::{ImpositionConfig, SheetOrientation};
use imposer::{ImposerError, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

slint::include_modules!();

pub const APP_ID: &str = "io.github.Franck-Demongin.Imposer";

pub fn run() -> Result<()> {
    let ui = AppWindow::new().map_err(|e| ImposerError::Generation(e.to_string()))?;

    if let Err(e) = slint::set_xdg_app_id(APP_ID) {
        eprintln!("avertissement : app_id non appliqué : {e}");
    }
    ui.set_app_title(format!("Imposer - v{}", env!("CARGO_PKG_VERSION")).into());
    let weak_ui = ui.as_weak();

    let busy_rx: Arc<Mutex<Option<BusyReceiver>>> = Arc::new(Mutex::new(None));

    let timer_weak = weak_ui.clone();
    let timer_rx = busy_rx.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(80),
        move || {
            let ui = match timer_weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            let mut guard = match timer_rx.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if let Some(rx) = guard.as_mut()
                && let Ok((status, output_path)) = rx.try_recv()
            {
                let level = if status.starts_with("Erreur") {
                    2
                } else if status.starts_with("PDF généré") {
                    1
                } else {
                    0
                };
                ui.set_status(status.into());
                ui.set_status_level(level);
                if let Some(path) = output_path {
                    ui.set_last_output_path(path.to_string_lossy().to_string().into());
                    ui.set_output_ready(true);
                }
                ui.set_busy(false);
                *guard = None;
            }
        },
    );
    let _timer = timer;

    let user_presets = imposer::presets::load_user_presets().unwrap_or_default();
    let save_operation_target = std::sync::Arc::new(std::sync::Mutex::new(None::<i32>));
    let save_operation_self_pending = std::sync::Arc::new(std::sync::Mutex::new(false));
    let save_operation_reset_consumed = std::sync::Arc::new(std::sync::Mutex::new(false));
    let save_operation_reset_window_active = std::sync::Arc::new(std::sync::Mutex::new(false));
    let sheet_selection_timers =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::<slint::Timer>::new()));
    let mut sheet_choices: Vec<(String, f32, f32)> = Vec::new();
    for size in imposer::config::SheetSize::builtins() {
        sheet_choices.push((size.name.clone(), size.width_mm, size.height_mm));
    }
    for preset in user_presets {
        sheet_choices.push((preset.name, preset.width_mm, preset.height_mm));
    }
    sheet_choices.push(("Personnalisé".to_string(), 210.0, 297.0));
    let sheet_choices_state = std::sync::Arc::new(std::sync::Mutex::new(sheet_choices.clone()));
    let sheet_labels: Vec<slint::SharedString> = sheet_choices
        .iter()
        .map(|(name, _, _)| name.clone().into())
        .collect();
    ui.set_sheet_options(slint::ModelRc::new(std::rc::Rc::new(
        slint::VecModel::from(sheet_labels),
    )));
    ui.set_sheet_selection_suppress_next(false);
    if let Some(first) = sheet_choices.first() {
        let (name, width, height) = first;
        ui.set_sheet_width_mm(format!("{:.1}", *width).into());
        ui.set_sheet_height_mm(format!("{:.1}", *height).into());
        ui.set_sheet_summary(
            format!("{} ({:.1} x {:.1} cm)", name, *width / 10.0, *height / 10.0,).into(),
        );
        ui.set_selected_sheet_name(name.clone().into());
        ui.set_sheet_save_visible(false);
        ui.set_sheet_delete_visible(false);
    }
    ui.set_custom_width_mm(format!("{:.1}", 210.0).into());
    ui.set_custom_height_mm(format!("{:.1}", 297.0).into());
    ui.set_previous_sheet_index(0);
    ui.set_custom_open(false);
    ui.set_custom_error("".into());
    ui.set_save_open(false);
    ui.set_preset_name_text("".into());
    ui.set_preset_save_error("".into());
    ui.set_sheet_index(0);

    {
        let weak = weak_ui.clone();
        ui.on_browse_source_one(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            if ui.get_busy() {
                return;
            }
            let picked = rfd::FileDialog::new()
                .add_filter("Sources", &["pdf", "png", "jpg", "jpeg", "psd"])
                .pick_file();
            let Some(path) = picked else {
                return;
            };
            let source = path.display().to_string();
            if ui.get_source_2() == source.as_str() {
                ui.set_status("Source déjà sélectionnée.".into());
                return;
            }
            ui.set_source_1(source.into());
            auto_fill_output(&ui, &path);
            refresh_preview(&ui);
        });
    }

    // Source 2
    {
        let weak = weak_ui.clone();
        ui.on_browse_source_two(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            if ui.get_busy() {
                return;
            }
            let picked = rfd::FileDialog::new()
                .add_filter("Sources", &["pdf", "png", "jpg", "jpeg", "psd"])
                .pick_file();
            let Some(path) = picked else {
                return;
            };
            let source = path.display().to_string();
            if ui.get_source_1() == source.as_str() {
                ui.set_status("Source déjà sélectionnée.".into());
                return;
            }
            ui.set_source_2(source.into());
            auto_fill_output(&ui, &path);
            refresh_preview(&ui);
        });
    }

    // Retirer la dernière
    {
        let weak = weak_ui.clone();
        ui.on_remove_last_source(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            if ui.get_busy() {
                return;
            }
            if !ui.get_source_2().to_string().is_empty() {
                ui.set_source_2("".into());
            } else {
                ui.set_source_1("".into());
            }
            refresh_preview(&ui);
        });
    }

    // Dossier de sortie
    {
        let weak = weak_ui.clone();
        ui.on_browse_output_directory(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            if ui.get_busy() {
                return;
            }
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                ui.set_output_dir(folder.display().to_string().into());
            }
        });
    }

    // Formats de feuilles personnalisés et préréglés
    {
        let weak = weak_ui.clone();
        let state = sheet_choices_state.clone();
        let save_target_cb = save_operation_target.clone();
        let save_self_pending_cb = save_operation_self_pending.clone();
        let save_reset_consumed_cb = save_operation_reset_consumed.clone();
        let save_reset_window_active_cb = save_operation_reset_window_active.clone();
        ui.on_sheet_selection_changed(move |selected_index: i32| {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            let save_target = *save_target_cb.lock().unwrap();
            let save_self_pending = *save_self_pending_cb.lock().unwrap();
            let save_reset_consumed = *save_reset_consumed_cb.lock().unwrap();
            let save_reset_window_active = *save_reset_window_active_cb.lock().unwrap();
            if let Some(expected) = save_target {
                let current_index = ui.get_sheet_index();

                if !save_reset_consumed && save_reset_window_active && selected_index != expected {
                    // The first asynchronous callback after saving is Slint's queued
                    // reset to A4, if it occurs within the reset window. Consume it
                    // once, restore the saved preset, and do not treat it as a user
                    // winning selection.
                    *save_reset_consumed_cb.lock().unwrap() = true;
                    ui.set_sheet_selection_lock(true);
                    ui.set_sheet_index(expected);
                    ui.set_sheet_selection_lock(false);
                    ui.set_sheet_selection_suppress_next(false);
                    return;
                }

                if !save_reset_consumed && selected_index == expected && save_self_pending {
                    // A save-generated callback has the same index as the saved
                    // preset. Consume the synchronous invocation without marking the
                    // asynchronous reset as handled, so a later queued reset can still
                    // be recognized.
                    *save_self_pending_cb.lock().unwrap() = false;
                    ui.set_sheet_selection_suppress_next(false);
                    return;
                }
                if save_reset_consumed && selected_index != current_index {
                    // Late queued callbacks are stale and must not hijack a genuine
                    // combo selection made after the reset has been consumed.
                    ui.set_sheet_selection_suppress_next(false);
                    return;
                }
            }

            // Clearing this flag must not swallow a genuine selection: the flag only
            // postpones the update until any queued callback from the save
            // operation has been consumed.
            if ui.get_sheet_selection_suppress_next() {
                ui.set_sheet_selection_suppress_next(false);
            }
            if selected_index != ui.get_sheet_index() {
                return;
            }
            let index = selected_index;
            if index < 0 {
                return;
            }
            let data = {
                let guard = state.lock().unwrap();
                if index as usize >= guard.len() {
                    return;
                }
                let builtin_count = imposer::config::SheetSize::builtins().len();
                let is_custom = index as usize == guard.len().saturating_sub(1);
                let is_user_preset = !is_custom
                    && index as usize >= builtin_count
                    && (index as usize) < guard.len().saturating_sub(1);
                let choice = if is_custom {
                    None
                } else {
                    Some(guard[index as usize].clone())
                };
                (is_custom, is_user_preset, choice)
            };
            if data.0 {
                let width = ui.get_sheet_width_mm().to_string();
                let height = ui.get_sheet_height_mm().to_string();
                ui.set_custom_width_mm(width.into());
                ui.set_custom_height_mm(height.into());
                ui.set_custom_error("".into());
                ui.set_custom_open(true);
                ui.set_selected_sheet_name("Personnalisé".into());
                ui.set_sheet_save_visible(true);
                ui.set_sheet_delete_visible(false);
            } else if let Some((name, width, height)) = data.2 {
                ui.set_sheet_width_mm(format!("{:.1}", width).into());
                ui.set_sheet_height_mm(format!("{:.1}", height).into());
                ui.set_sheet_summary(
                    format!("{} ({:.1} x {:.1} cm)", name, width / 10.0, height / 10.0).into(),
                );
                ui.set_previous_sheet_index(index);
                ui.set_custom_open(false);
                ui.set_selected_sheet_name(name.clone().into());
                ui.set_sheet_save_visible(false);
                ui.set_sheet_delete_visible(data.1);
            }
        });
    }
    {
        let weak = weak_ui.clone();
        ui.on_custom_confirm(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            let width = parse_slint_dimension(ui.get_custom_width_mm());
            let height = parse_slint_dimension(ui.get_custom_height_mm());
            match (width, height) {
                (Some(width), Some(height)) => {
                    ui.set_sheet_width_mm(format!("{:.1}", width).into());
                    ui.set_sheet_height_mm(format!("{:.1}", height).into());
                    ui.set_sheet_summary(
                        format!(
                            "Personnalisé ({:.1} x {:.1} cm)",
                            width / 10.0,
                            height / 10.0,
                        )
                        .into(),
                    );
                    ui.set_custom_error("".into());
                    ui.set_custom_open(false);
                    ui.set_selected_sheet_name("Personnalisé".into());
                    ui.set_sheet_save_visible(true);
                    ui.set_sheet_delete_visible(false);
                }
                _ => {
                    ui.set_custom_error(
                        "Dimensions invalides: valeurs numériques positives attendues.".into(),
                    );
                }
            }
        });
    }
    {
        let weak = weak_ui.clone();
        ui.on_custom_cancel(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            ui.set_custom_open(false);
            let previous = ui.get_previous_sheet_index();
            ui.set_sheet_index(previous);
        });
    }
    {
        let weak = weak_ui.clone();
        ui.on_save_preset_requested(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            ui.set_preset_name_text(
                format!(
                    "Format {:.0}x{:.0}",
                    parse_slint_dimension(ui.get_sheet_width_mm()).unwrap_or(210.0),
                    parse_slint_dimension(ui.get_sheet_height_mm()).unwrap_or(297.0),
                )
                .into(),
            );
            ui.set_preset_save_error("".into());
            ui.set_save_open(true);
        });
    }
    {
        let weak = weak_ui.clone();
        let state = sheet_choices_state.clone();
        ui.on_save_preset_confirm(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            let width = match parse_slint_dimension(ui.get_sheet_width_mm()) {
                Some(value) => value,
                None => {
                    ui.set_preset_save_error("Dimensions de feuille invalides.".into());
                    return;
                }
            };
            let height = match parse_slint_dimension(ui.get_sheet_height_mm()) {
                Some(value) => value,
                None => {
                    ui.set_preset_save_error("Dimensions de feuille invalides.".into());
                    return;
                }
            };
            let name = ui.get_preset_name_text().to_string();
            let builtins = imposer::config::SheetSize::builtins();
            let existing = imposer::presets::load_user_presets().unwrap_or_default();
            match imposer::presets::validate_user_preset(&name, width, height, &existing, &builtins)
            {
                Ok(user_preset) => {
                    let save_target_cb = save_operation_target.clone();
                    let save_self_pending_cb = save_operation_self_pending.clone();
                    let save_reset_consumed_cb = save_operation_reset_consumed.clone();
                    let save_reset_window_active_cb = save_operation_reset_window_active.clone();
                    ui.set_sheet_selection_lock(true);
                    ui.set_sheet_selection_suppress_next(true);
                    let selected_index = builtins.len() as i32 + existing.len() as i32;
                    *save_target_cb.lock().unwrap() = Some(selected_index);
                    *save_self_pending_cb.lock().unwrap() = true;
                    *save_reset_consumed_cb.lock().unwrap() = false;
                    *save_reset_window_active_cb.lock().unwrap() = true;
                    let mut updated = existing;
                    updated.push(user_preset.clone());
                    match imposer::presets::save_user_presets(&updated) {
                        Ok(()) => {
                            let mut rebuilt: Vec<(String, f32, f32)> = builtins
                                .into_iter()
                                .map(|preset| {
                                    (preset.name.clone(), preset.width_mm, preset.height_mm)
                                })
                                .collect();
                            for preset in &updated {
                                rebuilt.push((
                                    preset.name.clone(),
                                    preset.width_mm,
                                    preset.height_mm,
                                ));
                            }
                            rebuilt.push(("Personnalisé".to_string(), 210.0, 297.0));
                            {
                                let mut guard = state.lock().unwrap();
                                *guard = rebuilt.clone();
                            }
                            let labels: Vec<slint::SharedString> = rebuilt
                                .iter()
                                .map(|(name, _, _)| name.clone().into())
                                .collect();
                            ui.set_sheet_options(slint::ModelRc::new(std::rc::Rc::new(
                                slint::VecModel::from(labels),
                            )));
                            let selected_index = (rebuilt.len() as i32 - 1 - 1).max(0);

                            // The combo box is set directly while locked, avoiding a
                            // synchronous callback during the rebuild.
                            ui.set_sheet_index(selected_index);

                            // Keep the saved preset as the operation target.  The first
                            // queued asynchronous callback consumes the reset; ordinary
                            // selections after that point remain selectable.
                            *save_target_cb.lock().unwrap() = Some(selected_index);
                            *save_self_pending_cb.lock().unwrap() = true;
                            *save_reset_consumed_cb.lock().unwrap() = false;
                            *save_reset_window_active_cb.lock().unwrap() = true;

                            // Résoudre le format réellement ciblé à partir du préréglage
                            // reconstruit.  Ne pas supposer que la valeur actuelle de
                            // sheet-index (ni un déclenchement conditionnel du callback
                            // Slint) correspondra au bon préréglage : on met à jour ses
                            // champs directement avec ses dimensions et son nom.
                            let (target_name, target_width, target_height) =
                                rebuilt[selected_index as usize].clone();
                            ui.set_sheet_width_mm(format!("{:.1}", target_width).into());
                            ui.set_sheet_height_mm(format!("{:.1}", target_height).into());
                            ui.set_sheet_summary(
                                format!(
                                    "{} ({:.1} x {:.1} cm)",
                                    target_name,
                                    target_width / 10.0,
                                    target_height / 10.0,
                                )
                                .into(),
                            );
                            ui.set_status(
                                format!(
                                    "Format enregistré: {} ({:.0} x {:.0} mm).",
                                    user_preset.name, width, height
                                )
                                .into(),
                            );
                            ui.set_previous_sheet_index(selected_index);
                            ui.set_custom_open(false);
                            ui.set_save_open(false);
                            ui.set_selected_sheet_name(target_name.clone().into());
                            ui.set_sheet_save_visible(false);
                            ui.set_sheet_delete_visible(true);
                            ui.invoke_refresh_preview();
                            ui.set_sheet_selection_lock(false);

                            // Drive the operation synchronously rather than relying
                            // solely on an event-loop callback that may be reordered
                            // with a real user selection.  The callback below leaves
                            // the operation's target known but records that its
                            // first generated exact event has been consumed.
                            ui.invoke_sheet_selection_changed(selected_index);

                            // Clear only the immediate operation flags after a short
                            // delay.  `save_operation_target` itself remains useful
                            // so late stale callbacks can be distinguished from genuine
                            // selections.
                            let self_pending_clear = save_self_pending_cb.clone();
                            let window_clear = save_reset_window_active_cb.clone();
                            let weak = ui.as_weak();
                            let timers = sheet_selection_timers.clone();
                            let timer = slint::Timer::default();
                            timer.start(
                                slint::TimerMode::SingleShot,
                                std::time::Duration::from_millis(300),
                                move || {
                                    *self_pending_clear.lock().unwrap() = false;
                                    *window_clear.lock().unwrap() = false;
                                    if let Some(ui) = weak.upgrade() {
                                        ui.set_sheet_selection_suppress_next(false);
                                    }
                                },
                            );
                            timers.borrow_mut().push(timer);
                        }
                        Err(error) => {
                            ui.set_preset_save_error(error.into());
                        }
                    }
                }
                Err(error) => {
                    ui.set_preset_save_error(error.into());
                }
            }
        });
    }

    // Suppression des préréglages utilisateur
    {
        let weak = ui.as_weak();
        let state = sheet_choices_state.clone();
        ui.on_delete_preset_requested(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            if ui.get_busy() {
                return;
            }
            let index = ui.get_sheet_index();
            let (is_valid_user_preset, preset_name) = {
                let guard = state.lock().unwrap();
                if index < 0 || index as usize >= guard.len() {
                    return;
                }
                let builtin_count = imposer::config::SheetSize::builtins().len();
                let is_user_preset = index as usize >= builtin_count
                    && (index as usize) < guard.len().saturating_sub(1);
                let name = guard[index as usize].0.clone();
                (is_user_preset, name)
            };
            if !is_valid_user_preset {
                return;
            }
            ui.set_delete_preset_name(preset_name.into());
            ui.set_delete_preset_error("".into());
            ui.set_delete_open(true);
        });
    }
    {
        let weak = weak_ui.clone();
        let state = sheet_choices_state.clone();
        ui.on_delete_preset_confirm(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            if ui.get_busy() {
                return;
            }
            let index = ui.get_sheet_index();
            let target_name = {
                let guard = state.lock().unwrap();
                if index < 0 || index as usize >= guard.len() {
                    ui.set_delete_preset_error("Sélection invalide.".into());
                    return;
                }
                let builtin_count = imposer::config::SheetSize::builtins().len();
                if (index as usize) < builtin_count
                    || (index as usize) >= guard.len().saturating_sub(1)
                {
                    ui.set_delete_preset_error("Ce format ne peut pas être supprimé.".into());
                    return;
                }
                guard[index as usize].0.clone()
            };

            let existing = imposer::presets::load_user_presets().unwrap_or_default();
            let updated: Vec<imposer::presets::UserPreset> = existing
                .into_iter()
                .filter(|preset| !preset.name.eq_ignore_ascii_case(&target_name))
                .collect();
            if let Err(error) = imposer::presets::save_user_presets(&updated) {
                ui.set_delete_preset_error(error.into());
                return;
            }

            let builtins = imposer::config::SheetSize::builtins();
            let mut rebuilt: Vec<(String, f32, f32)> = builtins
                .into_iter()
                .map(|preset| (preset.name.clone(), preset.width_mm, preset.height_mm))
                .collect();
            for preset in &updated {
                rebuilt.push((preset.name.clone(), preset.width_mm, preset.height_mm));
            }
            rebuilt.push(("Personnalisé".to_string(), 210.0, 297.0));

            {
                let mut guard = state.lock().unwrap();
                *guard = rebuilt.clone();
            }
            let labels: Vec<slint::SharedString> = rebuilt
                .iter()
                .map(|(name, _, _)| name.clone().into())
                .collect();
            ui.set_sheet_options(slint::ModelRc::new(std::rc::Rc::new(
                slint::VecModel::from(labels),
            )));

            let (name, width, height) = rebuilt[0].clone();
            ui.set_sheet_index(0);
            ui.set_previous_sheet_index(0);
            ui.set_sheet_width_mm(format!("{:.1}", width).into());
            ui.set_sheet_height_mm(format!("{:.1}", height).into());
            ui.set_sheet_summary(
                format!("{} ({:.1} x {:.1} cm)", name, width / 10.0, height / 10.0).into(),
            );
            ui.set_selected_sheet_name(name.into());
            ui.set_sheet_save_visible(false);
            ui.set_sheet_delete_visible(false);
            ui.set_custom_open(false);
            ui.set_delete_open(false);
            ui.set_delete_preset_error("".into());
            ui.set_status(format!("Format supprimé: {}", target_name).into());
        });
    }

    // Génération
    {
        let weak = weak_ui.clone();
        let rx_state = busy_rx.clone();
        ui.on_generate(move || {
            let ui = match weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };
            if ui.get_busy() {
                ui.set_status("Génération déjà en cours...".into());
                return;
            }

            ui.set_output_ready(false);

            let mut source_paths = Vec::new();
            if !ui.get_source_1().to_string().trim().is_empty() {
                source_paths.push(PathBuf::from(ui.get_source_1().to_string()));
            }
            if !ui.get_source_2().to_string().trim().is_empty() {
                source_paths.push(PathBuf::from(ui.get_source_2().to_string()));
            }

            if source_paths.is_empty() {
                ui.set_status("Aucun fichier source sélectionné.".into());
                return;
            }

            let output_dir = ui.get_output_dir().to_string();
            let output_name = ui.get_output_name().to_string();
            let destination = match output_path(&output_dir, &output_name) {
                Some(path) => path,
                None => {
                    ui.set_status("Dossier ou nom de sortie invalide.".into());
                    return;
                }
            };

            let parent = destination
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            if !parent.as_os_str().is_empty()
                && !parent.exists()
                && let Err(e) = fs::create_dir_all(&parent)
            {
                ui.set_status(
                    format!("Impossible de créer le dossier {}: {e}", parent.display()).into(),
                );
                return;
            }

            let sheet_width_mm = match ui.get_sheet_width_mm().to_string().trim().parse::<f32>() {
                Ok(value) if value.is_finite() && value > 0.0 => value,
                _ => {
                    ui.set_status("Dimensions de feuille invalides.".into());
                    return;
                }
            };
            let sheet_height_mm = match ui.get_sheet_height_mm().to_string().trim().parse::<f32>() {
                Ok(value) if value.is_finite() && value > 0.0 => value,
                _ => {
                    ui.set_status("Dimensions de feuille invalides.".into());
                    return;
                }
            };
            let sheet_orientation = match ui.get_sheet_orientation_index() {
                1 => SheetOrientation::Portrait,
                2 => SheetOrientation::Landscape,
                _ => SheetOrientation::Auto,
            };
            let rotate_verso = ui.get_rotate_verso();
            let bleed = match ui.get_bleed_text().to_string().trim().parse::<f32>() {
                Ok(value) if value >= 0.0 => value,
                _ => {
                    ui.set_status("Bord perdu invalide.".into());
                    return;
                }
            };
            let safety = match ui.get_safety_text().to_string().trim().parse::<f32>() {
                Ok(value) if value >= 0.0 => value,
                _ => {
                    ui.set_status("Marge de sécurité invalide.".into());
                    return;
                }
            };
            let rotate = ui.get_rotate_copies();

            let (tx, rx) = std::sync::mpsc::channel();
            let thread_paths = source_paths.clone();
            let thread_destination = destination.clone();
            std::thread::spawn(move || {
                let status = run_imposition_work(
                    thread_paths,
                    destination,
                    sheet_width_mm,
                    sheet_height_mm,
                    sheet_orientation,
                    rotate_verso,
                    bleed,
                    safety,
                    rotate,
                );
                let output_path = if status.starts_with("PDF généré") {
                    Some(thread_destination)
                } else {
                    None
                };
                let _ = tx.send((status, output_path));
            });

            *rx_state.lock().unwrap() = Some(rx);
            ui.set_busy(true);
            ui.set_status("Exécution en cours...".into());
        });
    }

    {
        let weak = weak_ui.clone();
        ui.on_open_output_file(move || {
            if let Some(ui) = weak.upgrade() {
                let path = ui.get_last_output_path().to_string();
                if !path.trim().is_empty() {
                    let _ = imposer::open::open_path(std::path::Path::new(&path));
                }
            }
        });
    }

    {
        let weak = weak_ui.clone();
        ui.on_open_output_folder(move || {
            if let Some(ui) = weak.upgrade() {
                let path = ui.get_last_output_path().to_string();
                if !path.trim().is_empty() {
                    let folder = std::path::Path::new(&path)
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    let _ = imposer::open::open_path(folder);
                }
            }
        });
    }

    {
        let weak = weak_ui.clone();
        ui.on_refresh_preview(move || {
            if let Some(ui) = weak.upgrade() {
                refresh_preview(&ui);
            }
        });
    }

    refresh_preview(&ui);
    ui.set_status("Prêt. Sélectionnez jusqu'à 2 sources puis cliquez sur Générer.".into());

    ui.run()
        .map_err(|e| ImposerError::Generation(e.to_string()))?;
    Ok(())
}

fn current_preview_source(ui: &AppWindow) -> Option<String> {
    let first = ui.get_source_1().to_string();
    if !first.trim().is_empty() {
        return Some(first);
    }
    let second = ui.get_source_2().to_string();
    if !second.trim().is_empty() {
        Some(second)
    } else {
        None
    }
}

fn config_from_ui(ui: &AppWindow) -> Option<ImpositionConfig> {
    let sheet_width_mm = ui
        .get_sheet_width_mm()
        .to_string()
        .trim()
        .parse::<f32>()
        .ok()?;
    let sheet_height_mm = ui
        .get_sheet_height_mm()
        .to_string()
        .trim()
        .parse::<f32>()
        .ok()?;
    if !sheet_width_mm.is_finite()
        || sheet_width_mm <= 0.0
        || !sheet_height_mm.is_finite()
        || sheet_height_mm <= 0.0
    {
        return None;
    }
    let sheet_orientation = match ui.get_sheet_orientation_index() {
        1 => SheetOrientation::Portrait,
        2 => SheetOrientation::Landscape,
        _ => SheetOrientation::Auto,
    };
    let rotate_verso = ui.get_rotate_verso();
    let bleed = ui.get_bleed_text().to_string().trim().parse::<f32>().ok()?;
    let safety = ui
        .get_safety_text()
        .to_string()
        .trim()
        .parse::<f32>()
        .ok()?;
    if bleed < 0.0 || safety < 0.0 {
        return None;
    }
    Some(ImpositionConfig {
        sheet_width_mm,
        sheet_height_mm,
        sheet_orientation,
        bleed_mm: bleed,
        safety_margin_mm: safety,
        rotate_verso,
        rotate_copies: ui.get_rotate_copies(),
    })
}

fn refresh_preview(ui: &AppWindow) {
    let empty = slint::ModelRc::<PreviewPlacement>::default();
    let source_path = match current_preview_source(ui) {
        Some(path) => path,
        None => {
            ui.set_preview_width(0.0);
            ui.set_preview_height(0.0);
            ui.set_preview_placements(empty);
            return;
        }
    };

    let bytes = match std::fs::read(&source_path) {
        Ok(bytes) => bytes,
        _ => {
            ui.set_preview_width(0.0);
            ui.set_preview_height(0.0);
            ui.set_preview_placements(empty);
            return;
        }
    };

    let pages = match imposer::sources_from_bytes(std::slice::from_ref(&bytes)) {
        Ok(pages) => pages,
        Err(_) => {
            ui.set_preview_width(0.0);
            ui.set_preview_height(0.0);
            ui.set_preview_placements(empty);
            return;
        }
    };
    let source = match pages.first() {
        Some(page) => page,
        None => {
            ui.set_preview_width(0.0);
            ui.set_preview_height(0.0);
            ui.set_preview_placements(empty);
            return;
        }
    };

    let config = match config_from_ui(ui) {
        Some(config) => config,
        None => {
            ui.set_preview_width(0.0);
            ui.set_preview_height(0.0);
            ui.set_preview_placements(empty);
            return;
        }
    };

    let plan = match compute_layout(source.width_pt, source.height_pt, &config) {
        Ok(plan) => plan,
        Err(_) => {
            ui.set_preview_width(0.0);
            ui.set_preview_height(0.0);
            ui.set_preview_placements(empty);
            return;
        }
    };

    let target = 300.0;
    let scale = target / plan.sheet_width.max(plan.sheet_height).max(1.0);
    let width = plan.sheet_width * scale;
    let height = plan.sheet_height * scale;

    let rectangles: Vec<PreviewPlacement> = plan
        .placements
        .into_iter()
        .map(|placement| PreviewPlacement {
            x: placement.trim_x * scale,
            y: placement.trim_y * scale,
            w: placement.trim_w * scale,
            h: placement.trim_h * scale,
        })
        .collect();

    ui.set_preview_width(width);
    ui.set_preview_height(height);
    ui.set_preview_placements(slint::ModelRc::new(std::rc::Rc::new(
        slint::VecModel::from(rectangles),
    )));
}

fn auto_fill_output(ui: &AppWindow, path: &Path) {
    let parent = path
        .parent()
        .unwrap_or(Path::new("."))
        .display()
        .to_string();
    if ui.get_output_dir().to_string().trim().is_empty() {
        ui.set_output_dir(parent.into());
    }
    if ui.get_output_name().to_string().trim().is_empty() {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| String::from("output"));
        ui.set_output_name(format!("{}_imposed.pdf", stem).into());
    }
}

fn output_path(output_dir: &str, raw_name: &str) -> Option<PathBuf> {
    let directory = output_dir.trim();
    let raw_name = raw_name.trim();
    if directory.is_empty() || raw_name.is_empty() {
        return None;
    }
    let name_path = Path::new(raw_name);
    let components: Vec<_> = name_path.components().collect();
    if name_path.is_absolute()
        || components.is_empty()
        || components.len() != 1
        || raw_name == "."
        || raw_name == ".."
    {
        return None;
    }
    let mut name = raw_name.to_string();
    if !raw_name.to_ascii_lowercase().ends_with(".pdf") {
        name.push_str(".pdf");
    }
    Some(PathBuf::from(directory).join(name))
}

fn parse_slint_dimension(text: slint::SharedString) -> Option<f32> {
    let value = text
        .to_string()
        .replace(',', ".")
        .trim()
        .parse::<f32>()
        .ok()?;
    if value.is_finite() && value > 0.0 {
        Some(value)
    } else {
        None
    }
}
