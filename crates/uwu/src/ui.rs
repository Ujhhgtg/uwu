use std::sync::Arc;

use egui::{
    Align, Button, CentralPanel, Color32, Context, Id, LayerId, Layout, Pos2, Rect, RichText,
    Sense, Stroke, Ui, UiBuilder,
};
use wgpu::{Backend, PresentMode};
use winit::window::{Window, WindowLevel};

use crate::{
    assets,
    state::{
        AppCommand, AppState, CanvasImage, CanvasObject, CanvasObjectOps, CanvasShapeType,
        CanvasStroke, CanvasText, CanvasTool, DynamicBrushWidthMode, GraphicsApi, HistoryCommand,
        InsertTab, MarqueeMatchMode, ObjectTransform, OptimizationPolicy, PageState,
        PersistentState, PointerInteraction, PointerState, StrokeWidth, ThemeMode, WindowMode,
    },
    utils::{
        self, export,
        stroke::{brush_stroke_add_point, brush_stroke_end, brush_stroke_start},
        ui::{
            PageAction, UiExt, add_new_page_state, apply_theme_mode_and_canvas_color,
            apply_window_mode, clear_interaction_state, load_page_from_file, save_page_to_file,
            switch_to_page_state,
        },
    },
};

pub fn ui_welcome(state: &mut AppState, ctx: &Context) {
    let content_rect = ctx.content_rect();
    let center_pos = content_rect.center();

    let res = egui::Window::new("欢迎")
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .current_pos(center_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.heading("欢迎使用 uwu");
            ui.separator();

            ui.my_label("这是一个功能强大的数字画板应用，您可以：");
            ui.my_label("• 绘制和涂鸦");
            ui.my_label("• 使用各种工具进行编辑");
            ui.my_label("• 插入图片、文本和形状");
            ui.my_label("• 自定义画板设置");
            ui.my_label("• 保存与加载画布以保存你的工作");
            ui.my_label("• 导出画布为图片");
            ui.my_label("• 享受超快的启动速度与超高的流畅度");
            ui.separator();

            if ui.button("新建画布").clicked() {
                let default_page = PageState::default();
                state.pages = vec![default_page.clone()];
                state.current_page = 0;
                state.canvas = default_page.canvas;
                state.history = default_page.history;
                clear_interaction_state(state);
                state.show_welcome_window = false;
            }
            if ui.button("加载画布").clicked() {
                load_page_from_file(state, ctx);
            }

            ui.separator();

            ui.checkbox(
                &mut state.persistent.show_welcome_window_on_start,
                "启动时显示欢迎",
            );
        });
    if let Some(r) = res {
        state.egui_window_rects.push(r.response.rect);
    }
}

fn collapsing(ui: &mut Ui, section_id: &str, label: &str, add_body: impl FnOnce(&mut Ui)) {
    let id = ui.id().with(section_id);
    const ACTIVE_KEY: &str = "##toolbar_active";

    let active_value: u64 = ui
        .ctx()
        .data_mut(|d| d.get_persisted(egui::Id::new(ACTIVE_KEY)))
        .unwrap_or(u64::MAX);
    let section_value = id.value();

    let mut cs =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    let was_open = cs.is_open();

    if active_value != u64::MAX && active_value != section_value && was_open {
        cs.set_open(false);
        cs.store(ui.ctx());
    }

    let header_response;
    {
        let cs = &mut cs;
        let inner = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let toggle_resp =
                cs.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);

            let (label_rect, label_resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                egui::Sense::click(),
            );
            let text_color = if label_resp.hovered() {
                ui.style().visuals.widgets.active.text_color()
            } else {
                ui.style().visuals.widgets.noninteractive.text_color()
            };
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(label_rect)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.my_label(RichText::new(label).color(text_color));
                },
            );

            if label_resp.clicked() && !toggle_resp.clicked() {
                cs.toggle(ui);
                cs.store(ui.ctx());
            }

            toggle_resp
        });
        header_response = inner.response;
    }

    cs.show_body_indented(&header_response, ui, |ui| add_body(ui));

    let now_open =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
            .is_open();

    if now_open && !was_open {
        if active_value != u64::MAX && active_value != section_value {
            let mut prev_cs = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                egui::Id::new(active_value),
                false,
            );
            prev_cs.set_open(false);
            prev_cs.store(ui.ctx());
        }
        ui.ctx()
            .data_mut(|d| d.insert_persisted(egui::Id::new(ACTIVE_KEY), section_value));
    } else if !now_open && was_open && active_value == section_value {
        ui.ctx()
            .data_mut(|d| d.insert_persisted(egui::Id::new(ACTIVE_KEY), u64::MAX));
    }
}

pub fn ui_toolbar_settings(state: &mut AppState, ctx: &Context, ui: &mut Ui, window: &Arc<Window>) {
    collapsing(ui, "appearance", "外观", |ui| {
        ui.horizontal(|ui| {
            ui.my_label("画布颜色:");
            if ui
                .color_edit_button_srgba(&mut state.persistent.canvas_color)
                .changed()
                && !state.is_overlay_mode
            {
                apply_theme_mode_and_canvas_color(
                    ctx,
                    state.persistent.theme_mode,
                    state.persistent.canvas_color,
                );
            }
            if ui.button("重置").clicked() {
                state.persistent.canvas_color = utils::get_default_canvas_color();
                if !state.is_overlay_mode {
                    apply_theme_mode_and_canvas_color(
                        ctx,
                        state.persistent.theme_mode,
                        state.persistent.canvas_color,
                    );
                }
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("主题模式:");
            if ui
                .selectable_value(
                    &mut state.persistent.theme_mode,
                    ThemeMode::System,
                    "跟随系统",
                )
                .clicked()
                || ui
                    .selectable_value(
                        &mut state.persistent.theme_mode,
                        ThemeMode::Light,
                        "浅色模式",
                    )
                    .clicked()
                || ui
                    .selectable_value(
                        &mut state.persistent.theme_mode,
                        ThemeMode::Dark,
                        "深色模式",
                    )
                    .clicked()
            {
                apply_theme_mode_and_canvas_color(
                    ctx,
                    state.persistent.theme_mode,
                    state.persistent.canvas_color,
                );
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("启动时显示欢迎:");
            ui.checkbox(&mut state.persistent.show_welcome_window_on_start, "");
        });

        ui.horizontal(|ui| {
            ui.my_label("窗口透明度");
            ui.add(egui::Slider::new(
                &mut state.persistent.window_opacity,
                0.0..=1.0,
            ));
        });
    });

    collapsing(ui, "drawing", "绘制", |ui| {
        ui.horizontal(|ui| {
            ui.my_label("画布持久化:");
            if ui.button("加载").clicked() {
                load_page_from_file(state, ctx);
            }
            if ui.button("保存").clicked() {
                let page = PageState {
                    canvas: state.canvas.clone(),
                    history: state.history.clone(),
                    view_offset: state.view_offset,
                    view_zoom: state.view_zoom,
                };
                save_page_to_file(&mut state.toasts, &page);
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("画布文件关联:");
            if ui.button("安装").clicked() {
                match utils::associations::install_associations() {
                    Ok(_) => {
                        state.toasts.success("文件关联安装成功!");
                    }
                    Err(e) => {
                        state.toasts.error(format!("文件关联安装失败: {}", e));
                    }
                }
            }
            if ui.button("卸载").clicked() {
                match utils::associations::uninstall_associations() {
                    Ok(_) => {
                        state.toasts.success("文件关联卸载成功!");
                    }
                    Err(e) => {
                        state.toasts.error(format!("文件关联卸载失败: {}", e));
                    }
                }
            }
            let is_installed = utils::associations::is_associations_installed();
            if is_installed {
                ui.label(
                    egui::RichText::new("✓ 已安装").color(egui::Color32::from_rgb(40, 200, 40)),
                );
            } else {
                ui.label(
                    egui::RichText::new("X 未安装").color(egui::Color32::from_rgb(220, 50, 50)),
                );
            }
        });

        if !state.is_overlay_mode {
            ui.horizontal(|ui| {
                ui.my_label("画布导出:");
                if ui.button("单页导出为位图").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("位图", IMAGE_FILE_EXTS)
                        .set_file_name("canvas_page.bmp")
                        .save_file()
                {
                    state.screenshot_path = Some(path);
                }

                if ui.button("单页导出为 SVG").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("SVG 矢量图", &["svg"])
                        .set_file_name("canvas_page.svg")
                        .save_file()
                {
                    match export::export_page_to_svg(
                        &state.canvas,
                        state.persistent.canvas_color,
                        &path,
                    ) {
                        Ok(_) => {
                            state.toasts.success("画布导出成功!");
                        }
                        Err(err) => {
                            state.toasts.error(format!("画布导出失败: {}", err));
                        }
                    }
                }

                if ui.button("所有页导出为 PDF").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("PDF 文档", &["pdf"])
                        .set_file_name("canvas.pdf")
                        .save_file()
                {
                    let mut pages_canvas = Vec::new();
                    for i in 0..state.pages.len() {
                        if i == state.current_page {
                            pages_canvas.push(&state.canvas);
                        } else {
                            pages_canvas.push(&state.pages[i].canvas);
                        }
                    }

                    match export::export_all_pages_to_pdf(
                        &pages_canvas,
                        state.persistent.canvas_color,
                        &path,
                    ) {
                        Ok(_) => {
                            state.toasts.success("画布导出成功!");
                        }
                        Err(err) => {
                            state.toasts.error(format!("画布导出失败: {}", err));
                        }
                    }
                }
            });
        }

        ui.horizontal(|ui| {
            ui.my_label("动态画笔宽度微调:");
            ui.selectable_value(
                &mut state.dynamic_brush_width_mode,
                DynamicBrushWidthMode::Disabled,
                "禁用",
            );
            ui.selectable_value(
                &mut state.dynamic_brush_width_mode,
                DynamicBrushWidthMode::BrushTip,
                "模拟笔锋",
            );
            ui.selectable_value(
                &mut state.dynamic_brush_width_mode,
                DynamicBrushWidthMode::SpeedBased,
                "基于速度",
            );
        });

        ui.horizontal(|ui| {
            ui.my_label("笔迹平滑:");
            ui.checkbox(&mut state.persistent.stroke_smoothing, "");
        });

        ui.horizontal(|ui| {
            ui.my_label("直线停留拉直:");
            ui.checkbox(&mut state.persistent.stroke_straightening, "");
            if state.persistent.stroke_straightening {
                ui.my_label("灵敏度:");
                ui.add(egui::Slider::new(
                    &mut state.persistent.stroke_straightening_tolerance,
                    1.0..=50.0,
                ));
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("插值频率:");
            ui.add(egui::Slider::new(
                &mut state.persistent.interpolation_frequency,
                0.0..=1.0,
            ));
        });

        ui.horizontal(|ui| {
            ui.my_label("低延迟模式:");
            ui.checkbox(&mut state.persistent.low_latency_mode, "");
        });

        #[cfg(windows)]
        ui.horizontal(|ui| {
            ui.my_label("禁用系统边缘手势:");
            let resp = ui.checkbox(&mut state.persistent.disable_edge_gestures, "");
            if resp.changed() {
                let is_fullscreen = matches!(
                    state.persistent.window_mode,
                    WindowMode::ExclusiveFullscreen | WindowMode::BorderlessFullscreen
                );
                if is_fullscreen {
                    let hwnd = utils::windows::winit_window_to_hwnd(window);
                    if let Some(hwnd) = hwnd {
                        if utils::windows::is_windows_10_or_greater() {
                            unsafe {
                                let _ = utils::windows::disable_edge_gestures(
                                    hwnd,
                                    state.persistent.disable_edge_gestures,
                                );
                            }
                        }
                    }
                }
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("编辑快捷颜色:");
            if ui.button("OK").clicked() {
                state.show_quick_color_edit_window = true;
            }
        });

        // 快捷颜色编辑器窗口
        if state.show_quick_color_edit_window {
            let content_rect = ctx.content_rect();
            let center_pos = content_rect.center();

            egui::Window::new("编辑快捷颜色")
                .collapsible(false)
                .resizable(false)
                .movable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos([center_pos.x, center_pos.y])
                .show(ctx, |ui| {
                    ui.my_label("当前快捷颜色:");
                    ui.separator();

                    // 显示当前快捷颜色列表
                    let mut color_index_to_remove = None;
                    for (index, color) in state.persistent.quick_colors.iter().enumerate() {
                        ui.horizontal(|ui| {
                            // 创建一个临时可变副本用于颜色编辑器
                            let mut temp_color = *color;
                            ui.color_edit_button_srgba(&mut temp_color);
                            if ui.button("删除").clicked() {
                                color_index_to_remove = Some(index);
                            }
                        });
                    }

                    // 处理删除操作
                    if let Some(index) = color_index_to_remove {
                        state.persistent.quick_colors.remove(index);
                    }

                    ui.separator();

                    // 添加新颜色
                    ui.horizontal(|ui| {
                        ui.my_label("新颜色:");
                        ui.color_edit_button_srgba(&mut state.new_quick_color);
                        if ui.button("添加").clicked() {
                            state.persistent.quick_colors.push(state.new_quick_color);
                            state.new_quick_color = Color32::WHITE;
                        }
                    });

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("完成").clicked() {
                            state.show_quick_color_edit_window = false;
                        }
                        if ui.button("重置").clicked() {
                            state.show_quick_color_edit_window = false;
                            state.persistent.quick_colors = utils::get_default_quick_colors();
                        }
                    });
                });
        }
    });

    collapsing(ui, "performance", "性能", |ui| {
        ui.horizontal(|ui| {
            ui.my_label("窗口模式:");
            if ui
                .selectable_value(
                    &mut state.persistent.window_mode,
                    WindowMode::Windowed,
                    "窗口化",
                )
                .changed()
                || {
                    let response = ui.add_enabled(
                        !state.fullscreen_video_modes.is_empty(),
                        Button::selectable(
                            state.persistent.window_mode == WindowMode::ExclusiveFullscreen,
                            "独占全屏",
                        ),
                    );
                    if response.clicked()
                        && state.persistent.window_mode != WindowMode::ExclusiveFullscreen
                    {
                        state.persistent.window_mode = WindowMode::ExclusiveFullscreen;
                        true
                    } else {
                        false
                    }
                }
                || ui
                    .selectable_value(
                        &mut state.persistent.window_mode,
                        WindowMode::BorderlessFullscreen,
                        "无边框全屏",
                    )
                    .changed()
            {
                apply_window_mode(state, window);
            }
        });

        // 显示模式选择（仅在全屏模式下可用）
        ui.horizontal(|ui| {
            ui.my_label("显示模式:");

            // 显示当前选择的视频模式
            if state.persistent.window_mode == WindowMode::ExclusiveFullscreen {
                let mut current_selection = state.selected_video_mode_index.unwrap_or(0);

                let mode = state
                    .fullscreen_video_modes
                    .get(current_selection)
                    .expect("no video mode available");

                let mode_text = format!(
                    "{}x{} @ {}Hz",
                    mode.size().width,
                    mode.size().height,
                    mode.refresh_rate_millihertz() as f32 / 1000.0
                );

                egui::ComboBox::from_id_salt("video_mode_selection")
                    .selected_text(mode_text)
                    .show_ui(ui, |ui| {
                        for (index, mode) in state.fullscreen_video_modes.clone().iter().enumerate()
                        {
                            let mode_text = format!(
                                "{}x{} @ {}Hz",
                                mode.size().width,
                                mode.size().height,
                                mode.refresh_rate_millihertz() as f32 / 1000.0
                            );
                            if ui
                                .selectable_value(&mut current_selection, index, mode_text)
                                .changed()
                            {
                                state.selected_video_mode_index = Some(current_selection);
                                apply_window_mode(state, window);
                            }
                        }
                    });
            } else {
                ui.my_label(egui::RichText::new("(仅在独占全屏模式下可切换)").italics());
            }
        });

        // 垂直同步模式选择
        ui.horizontal(|ui| {
            ui.my_label("垂直同步:");
            if ui
                .selectable_value(
                    &mut state.persistent.present_mode,
                    PresentMode::AutoVsync,
                    "开 (自动) | AutoVsync",
                )
                .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::AutoNoVsync,
                        "关 (自动) | AutoNoVsync",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::Fifo,
                        "开 | Fifo",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::FifoRelaxed,
                        "自适应 | FifoRelaxed",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::Immediate,
                        "关 | Immediate",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::Mailbox,
                        "开 (快速) | Mailbox",
                    )
                    .changed()
            {
                state
                    .command_queue
                    .push(AppCommand::SetPresentMode(state.persistent.present_mode));
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("优化策略 [需重启以应用]:");
            ui.selectable_value(
                &mut state.persistent.optimization_policy,
                OptimizationPolicy::Performance,
                "性能",
            );
            ui.selectable_value(
                &mut state.persistent.optimization_policy,
                OptimizationPolicy::ResourceUsage,
                "资源用量",
            );
        });

        let current_backend = state.active_backend.unwrap_or(Backend::Noop);
        ui.horizontal(|ui| {
            ui.my_label("图形 API [需重启以应用]:");
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Auto,
                "自动",
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Vulkan,
                if current_backend == Backend::Vulkan {
                    "Vulkan (当前)"
                } else {
                    "Vulkan"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Dx12,
                if current_backend == Backend::Dx12 {
                    "Dx12 (当前)"
                } else {
                    "Dx12"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Metal,
                if current_backend == Backend::Metal {
                    "Metal (当前)"
                } else {
                    "Metal"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::WebGpu,
                if current_backend == Backend::BrowserWebGpu {
                    "WebGPU (当前)"
                } else {
                    "WebGPU"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Gl,
                if current_backend == Backend::Gl {
                    "Gl (当前)"
                } else {
                    "Gl"
                },
            );
        });

        ui.horizontal(|ui| {
            ui.my_label("强制每帧重绘:");
            ui.checkbox(&mut state.persistent.force_redraw_every_frame, "");
        });
    });

    collapsing(ui, "debug", "调试", |ui| {
        ui.horizontal(|ui| {
            ui.my_label("引发异常:");
            if ui.button("OK").clicked() {
                panic!("test panic")
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("显示 FPS:");
            ui.checkbox(&mut state.persistent.show_fps, "");
        });

        ui.horizontal(|ui| {
            ui.my_label("显示触控点:");
            ui.checkbox(&mut state.show_touch_points, "");
        });

        ui.horizontal(|ui| {
            ui.my_label("压力测试:");
            if ui.button("OK").clicked() {
                // 使用固定颜色和宽度
                const STRESS_COLOR: Color32 = Color32::from_rgb(255, 0, 0); // 红色
                const STRESS_WIDTH: f32 = 3.0;

                // 添加 1000 条笔画
                for i in 0..1000 {
                    let mut points = Vec::with_capacity(100);

                    // 生成笔画位置
                    let start_x = (i as f32 % 20.0) * 50.0;
                    let start_y = ((i as f32 / 20.0).floor() % 15.0) * 50.0;

                    // 生成笔画方向和长度
                    for j in 0..100 {
                        let x = start_x + (j as f32 * 10.0);
                        let y = start_y + (j as f32 * 5.0);

                        points.push(Pos2::new(x, y));
                    }

                    // 创建笔画对象
                    let stroke = CanvasStroke {
                        points,
                        width: STRESS_WIDTH.into(),
                        color: STRESS_COLOR,
                        base_width: STRESS_WIDTH,
                        shape: None,
                    };

                    state.canvas.objects.push(CanvasObject::Stroke(stroke));
                }
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("立即保存设置:");
            if ui.button("OK").clicked()
                && let Err(err) = state.persistent.save_to_file()
            {
                state.toasts.error(format!("设置保存失败: {}!", err));
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("重置设置:");
            if ui.button("OK").clicked() {
                clear_interaction_state(state);
                state.persistent = PersistentState::default();
                if !state.is_overlay_mode {
                    apply_theme_mode_and_canvas_color(
                        ctx,
                        state.persistent.theme_mode,
                        state.persistent.canvas_color,
                    );
                }
                state
                    .command_queue
                    .push(AppCommand::SetPresentMode(state.persistent.present_mode));
                apply_window_mode(state, window);
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("???:");
            ui.checkbox(&mut state.persistent.easter_egg_redo, "");
        });
    });

    collapsing(ui, "plugins", "插件", |ui| {
        let mut paths_to_remove: Vec<std::path::PathBuf> = Vec::new();
        for plugin in &state.plugins {
            let path = plugin.path.clone();
            ui.horizontal(|ui| {
                ui.my_label(format!(
                    "{} v{} ({})",
                    plugin.name, plugin.version, plugin.id
                ));
                if state.persistent.plugin_paths.contains(&plugin.path) && ui.button("X").clicked()
                {
                    paths_to_remove.push(path);
                }
            });
        }
        for path in &paths_to_remove {
            state.persistent.plugin_paths.retain(|p| p != path);
        }

        if !state.plugins.is_empty() {
            ui.separator();
        }

        if ui.button("加载插件").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("动态库", &["so", "dylib", "dll"])
                .pick_file()
        {
            // defer loading to avoid borrow issues;
            // push a command that AppState::load_plugin handles on the next redraw
            state.command_queue.push(AppCommand::LoadPlugin(path));
        }

        // FIXME: exiting after doing this triggers a SIGSEGV on linux
        // if ui.button("卸载所有插件").clicked() {
        //     state.command_queue.push(AppCommand::UnloadAllPlugins);
        // }
    });

    collapsing(ui, "about", "关于", |ui| {
        ui.my_label("uwu (ujhhgtg's whiteboard, unleashed)");
        ui.my_label(format!("版本: {}", env!("CARGO_PKG_VERSION")));
        ui.my_label(format!("作者: {}", env!("CARGO_PKG_AUTHORS")));
    });
}
pub fn ui_history(state: &mut AppState, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.my_label("历史记录:");
        if ui.button("撤销").clicked() {
            state.clear_selection();
            if state.history.undo(&mut state.canvas) {
                state.toasts.success("成功撤销操作!");
            } else {
                state.toasts.error("无法撤销，没有更多历史记录!");
            }
        }
        if ui
            .button(if !state.persistent.easter_egg_redo {
                "重做"
            } else {
                "Redo!"
            })
            .clicked()
        {
            state.clear_selection();
            if state.history.redo(&mut state.canvas) {
                state.toasts.success("成功重做操作!");
            } else {
                state.toasts.error("无法重做，没有更多历史记录!");
            }
        }
    });
}

pub fn ui_window_controls(state: &mut AppState, ui: &mut Ui, window: &Arc<Window>) {
    ui.horizontal(|ui| {
        if ui.button("退出").clicked() {
            state.should_quit = true;
        }

        if ui.button("最小化").clicked() {
            window.set_minimized(true);
        }

        ui.horizontal(|ui| {
            ui.my_label("悬浮窗模式:");
            if ui.checkbox(&mut state.is_overlay_mode, "").changed() {
                state.command_queue.push(AppCommand::UpdateCursorHittest);
                if state.is_overlay_mode {
                    window.set_window_level(WindowLevel::AlwaysOnTop);
                    state.current_tool = CanvasTool::Passthrough;
                } else {
                    window.set_window_level(WindowLevel::Normal);
                    state.current_tool = CanvasTool::Brush;
                }
                clear_interaction_state(state);
                apply_theme_mode_and_canvas_color(
                    ui.ctx(),
                    state.persistent.theme_mode,
                    if state.is_overlay_mode {
                        Color32::TRANSPARENT
                    } else {
                        state.persistent.canvas_color
                    },
                );
            }
        });

        if state.persistent.show_fps {
            ui.my_label(format!("FPS: {}", state.fps_counter.current_fps));
        }

        #[cfg(windows)]
        {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("屏幕键盘").clicked() {
                    const TABTIP_REL: &str =
                        r"\Program Files\Common Files\microsoft shared\ink\TabTip.exe";

                    let running = std::process::Command::new("tasklist")
                        .args(["/fi", "imagename eq TabTip.exe", "/nh"])
                        .output()
                        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("TabTip.exe"));

                    if !running {
                        use std::{env, ffi::OsStr, os::windows::ffi::OsStrExt};

                        use windows::{Win32::UI::Shell::ShellExecuteW, core::PCWSTR};

                        let target = format!(
                            "{}{}",
                            env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string()),
                            TABTIP_REL
                        );

                        let target_w: Vec<u16> = OsStr::new(&target)
                            .encode_wide()
                            .chain(std::iter::once(0))
                            .collect();

                        let verb_w: Vec<u16> = OsStr::new("open")
                            .encode_wide()
                            .chain(std::iter::once(0))
                            .collect();

                        let hinst = unsafe {
                            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

                            // this prevents the '请求的操作需要提升。 (os error 740)' error
                            ShellExecuteW(
                                None,
                                PCWSTR(verb_w.as_ptr()),
                                PCWSTR(target_w.as_ptr()),
                                PCWSTR::null(),
                                PCWSTR::null(),
                                SW_SHOWNORMAL,
                            )
                        };

                        if hinst.0 as usize <= 32 {
                            eprintln!("ShellExecuteW failed: {:?}", hinst);
                        }

                        return;
                    }

                    let hwnd = utils::windows::winit_window_to_hwnd(window);
                    let _ = utils::windows::toggle_touch_keyboard(hwnd);
                }
            });
        }
    });
}

pub fn ui_pages_nav(state: &mut AppState, ctx: &Context) -> Option<(Rect, Rect)> {
    let content_rect = ctx.content_rect();
    let margin = 8.0;
    let total_pages = state.pages.len();
    let current = state.current_page;
    let enabled = !state.show_welcome_window;

    if enabled {
        let mut action = PageAction::None;

        let build_page_nav =
            |ui: &mut Ui, action: &mut PageAction, show_management_window: &mut bool| {
                let btn_style = |text: &str| {
                    egui::Button::new(egui::RichText::new(text).size(20.0))
                        .min_size(egui::vec2(36.0, 28.0))
                };
                ui.horizontal(|ui| {
                    if ui.add_enabled(current > 0, btn_style("<")).clicked() {
                        *action = PageAction::Previous;
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("{}/{}", current + 1, total_pages))
                                    .size(20.0),
                            )
                            .min_size(egui::vec2(48.0, 28.0)),
                        )
                        .clicked()
                    {
                        *show_management_window = true;
                    }

                    let is_last = current == total_pages - 1;
                    if is_last {
                        if ui.add(btn_style("+")).clicked() {
                            *action = PageAction::New;
                        }
                    } else if ui.add(btn_style(">")).clicked() {
                        *action = PageAction::Next;
                    }
                });
            };

        // left-bottom window
        let win1 = egui::Window::new("##page_nav_left")
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .title_bar(false)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .current_pos(Pos2::new(
                content_rect.min.x + margin,
                content_rect.max.y - margin,
            ))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let mut a = PageAction::None;
                build_page_nav(ui, &mut a, &mut state.show_page_management_window);
                if !matches!(a, PageAction::None) {
                    action = a;
                }
            })
            .unwrap()
            .response
            .rect;

        // right-bottom window
        let win2 = egui::Window::new("##page_nav_right")
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .title_bar(false)
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .current_pos(Pos2::new(
                content_rect.max.x - margin,
                content_rect.max.y - margin,
            ))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let mut a = PageAction::None;
                build_page_nav(ui, &mut a, &mut state.show_page_management_window);
                if !matches!(a, PageAction::None) {
                    action = a;
                }
            })
            .unwrap()
            .response
            .rect;

        apply_page_action(state, action);

        return Some((win1, win2));
    }

    None
}

fn apply_page_action(state: &mut AppState, action: PageAction) {
    match action {
        PageAction::Previous if state.current_page > 0 => {
            switch_to_page_state(state, state.current_page - 1);
        }
        PageAction::Next if state.current_page + 1 < state.pages.len() => {
            switch_to_page_state(state, state.current_page + 1);
        }
        PageAction::New => {
            add_new_page_state(state);
        }
        _ => {}
    }
}

pub fn ui_pages_manager(state: &mut AppState, ctx: &Context) {
    let content_rect = ctx.content_rect();
    let center_pos = content_rect.center();
    let total_pages = state.pages.len();

    let res = egui::Window::new(format!("页面管理 (共 {} 页)", total_pages))
        .id("page_man".into())
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .current_pos(center_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let mut pages_to_remove: Vec<usize> = Vec::new();

            let scroll_height = (total_pages as f32 * 50.0).min(300.0);
            egui::ScrollArea::vertical()
                .max_height(scroll_height)
                .show(ui, |ui| {
                    let mut dnd_from: Option<usize> = None;
                    let mut dnd_to: Option<usize> = None;

                    let zone_frame = egui::Frame::NONE.inner_margin(4.0);
                    let (_, dropped_payload) = ui.dnd_drop_zone::<usize, ()>(zone_frame, |ui| {
                        ui.set_min_width(ui.available_width());

                        let mut i = 0;
                        while i < state.pages.len() {
                            let is_current = i == state.current_page;

                            let row_frame = egui::Frame::NONE
                                .fill(ui.visuals().window_fill)
                                .inner_margin(egui::Margin::symmetric(8, 3));

                            let row_response = row_frame
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.set_min_height(36.0);

                                        let handle_id = egui::Id::new(("page_drag_handle", i));
                                        let _ = ui.dnd_drag_source(handle_id, i, |ui| {
                                            ui.my_label(egui::RichText::new("-").size(16.0));
                                        });

                                        if is_current {
                                            ui.my_label(
                                                egui::RichText::new(format!("第 {} 页", i + 1))
                                                    .strong(),
                                            );
                                        } else {
                                            ui.my_label(format!("第 {} 页", i + 1));
                                        }

                                        if ui.button("✓ 保存").clicked() {
                                            save_page_to_file(&mut state.toasts, &state.pages[i]);
                                        }

                                        if ui
                                            .add_enabled(
                                                total_pages > 1,
                                                egui::Button::new("X 删除"),
                                            )
                                            .clicked()
                                        {
                                            pages_to_remove.push(i);
                                        }

                                        if ui
                                            .add_enabled(
                                                !is_current,
                                                egui::Button::new(if !is_current {
                                                    "→ 跳转"
                                                } else {
                                                    "⊙ 当前"
                                                }),
                                            )
                                            .clicked()
                                        {
                                            switch_to_page_state(state, i);
                                        }
                                    });
                                })
                                .response;

                            if let (Some(pointer), Some(hovered_payload)) = (
                                ui.input(|i| i.pointer.interact_pos()),
                                row_response.dnd_hover_payload::<usize>(),
                            ) {
                                let rect = row_response.rect;
                                let stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
                                if *hovered_payload == i {
                                    ui.painter().hline(rect.x_range(), rect.center().y, stroke);
                                } else if pointer.y < rect.center().y {
                                    ui.painter().hline(rect.x_range(), rect.top(), stroke);
                                } else {
                                    ui.painter().hline(rect.x_range(), rect.bottom(), stroke);
                                }

                                if let Some(dragged_payload) =
                                    row_response.dnd_release_payload::<usize>()
                                {
                                    let insert_row_idx = if pointer.y < rect.center().y {
                                        i
                                    } else {
                                        i + 1
                                    };
                                    dnd_from = Some(*dragged_payload);
                                    dnd_to = Some(insert_row_idx);
                                }
                            }
                            i += 1;
                        }
                    });

                    if let Some(dragged_payload) = dropped_payload {
                        dnd_from = Some(*dragged_payload);
                        dnd_to = Some(usize::MAX);
                    }

                    // Apply reorder
                    if let (Some(from_idx), Some(to_idx)) = (dnd_from, dnd_to) {
                        let old_cp = state.current_page;
                        std::mem::swap(&mut state.canvas, &mut state.pages[old_cp].canvas);
                        std::mem::swap(&mut state.history, &mut state.pages[old_cp].history);

                        let page = state.pages.remove(from_idx);

                        let insert_at = if to_idx == usize::MAX || to_idx >= state.pages.len() {
                            state.pages.len()
                        } else if to_idx > from_idx {
                            to_idx - 1
                        } else {
                            to_idx
                        };
                        let insert_at = insert_at.min(state.pages.len());

                        state.pages.insert(insert_at, page);

                        state.current_page = if old_cp == from_idx {
                            insert_at
                        } else if old_cp > from_idx && old_cp <= insert_at {
                            old_cp - 1
                        } else if old_cp < from_idx && old_cp >= insert_at {
                            old_cp + 1
                        } else {
                            old_cp
                        };

                        let cur = state.current_page;
                        std::mem::swap(&mut state.canvas, &mut state.pages[cur].canvas);
                        std::mem::swap(&mut state.history, &mut state.pages[cur].history);
                    }
                });

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("+ 新页").clicked() {
                    add_new_page_state(state);
                }
                if ui.button("O 加载").clicked() {
                    load_page_from_file(state, ctx);
                }
                if ui.button("X 关闭").clicked() {
                    state.show_page_management_window = false;
                }
            });

            // Apply deletions
            if !pages_to_remove.is_empty() {
                pages_to_remove.sort();
                pages_to_remove.dedup();
                let old = state.current_page;
                std::mem::swap(&mut state.canvas, &mut state.pages[old].canvas);
                std::mem::swap(&mut state.history, &mut state.pages[old].history);
                for &i in pages_to_remove.iter().rev() {
                    state.pages.remove(i);
                    if state.current_page >= i && state.current_page > 0 {
                        state.current_page -= 1;
                    }
                }
                if state.current_page >= state.pages.len() {
                    state.current_page = state.pages.len() - 1;
                }
                let cur = state.current_page;
                std::mem::swap(&mut state.canvas, &mut state.pages[cur].canvas);
                std::mem::swap(&mut state.history, &mut state.pages[cur].history);
                clear_interaction_state(state);
            }
        });
    if let Some(r) = res {
        state.egui_window_rects.push(r.response.rect);
    }
}

fn ui_toolbar_tools_content(
    state: &mut AppState,
    ctx: &Context,
    ui: &mut Ui,
    window: &Arc<Window>,
) {
    if state.current_tool == CanvasTool::Passthrough {
        ui.my_label(egui::RichText::new("(当前处于穿透模式, 输入将穿透画布)").italics());
    } else if state.current_tool == CanvasTool::View {
        ui.my_label(egui::RichText::new("(在画布上滑动以移动视图, 滚轮或双指缩放)").italics());
        ui.horizontal(|ui| {
            if ui.button("全部重置").clicked() {
                state.view_offset = Default::default();
                state.view_zoom = 1.0;
            }
            ui.my_label("移动:");
            if ui.button("重置").clicked() {
                state.view_offset = Default::default();
            }
            ui.my_label("缩放:");
            let mut zoom = state.view_zoom;
            let slider = ui.add(
                egui::Slider::new(
                    &mut zoom,
                    crate::state::AppState::MIN_ZOOM..=crate::state::AppState::MAX_ZOOM,
                )
                .logarithmic(true),
            );
            if slider.changed() {
                let s = ui.ctx().content_rect().center();
                state.view_offset += s.to_vec2() * (1.0 / state.view_zoom - 1.0 / zoom);
                state.view_zoom = zoom;
            }
            if ui.button("重置").clicked() {
                let s = ui.ctx().content_rect().center();
                state.view_offset += s.to_vec2() * (1.0 / state.view_zoom - 1.0);
                state.view_zoom = 1.0;
            }
        });
    } else if state.current_tool == CanvasTool::Select {
        ui.horizontal(|ui| {
            ui.my_label("圈选匹配模式:");
            ui.selectable_value(
                &mut state.marquee_match_mode,
                MarqueeMatchMode::Overlapping,
                "重叠",
            );
            ui.selectable_value(
                &mut state.marquee_match_mode,
                MarqueeMatchMode::Containing,
                "包含",
            );
        });
        ui.checkbox(
            &mut state.persistent.click_or_drag_to_single_select,
            "未选中对象时允许点击或拖动以单选对象",
        );

        if !state.selected_object_indices.is_empty() {
            ui.horizontal(|ui| {
                ui.my_label("对象操作:");
                if ui.button("删除").clicked() {
                    // Delete all selected, batch history (reverse to keep indices stable)
                    let mut removed_objects = Vec::new();
                    let mut indices: Vec<usize> = state.selected_object_indices.clone();
                    indices.sort_unstable();
                    for &idx in indices.iter().rev() {
                        if idx < state.canvas.objects.len() {
                            let obj = state.canvas.objects.remove(idx);
                            removed_objects.push((idx, obj));
                        }
                    }
                    state.clear_selection();
                    let commands: Vec<HistoryCommand> = removed_objects
                        .into_iter()
                        .map(|(idx, obj)| HistoryCommand::RemoveObject {
                            index: idx,
                            object: obj,
                        })
                        .collect();
                    state.history.save_batch(commands);
                    state.toasts.success("对象已删除!");
                }
                if ui.button("复制").clicked() {
                    // Clone all selected, offset each by 20,20
                    let mut new_indices = Vec::new();
                    for &idx in &state.selected_object_indices.clone() {
                        if idx < state.canvas.objects.len() {
                            let mut clone = state.canvas.objects[idx].clone();
                            CanvasObject::move_object(&mut clone, egui::vec2(20.0, 20.0));
                            let new_idx = state.canvas.objects.len();
                            state.history.save_add_object(new_idx, clone.clone());
                            state.canvas.objects.push(clone);
                            new_indices.push(new_idx);
                        }
                    }
                    state.selected_object_indices = new_indices;
                    state.toasts.success("对象已复制!");
                }
                if ui.button("置顶").clicked() {
                    // Move all selected to end, preserving relative order
                    let mut indices: Vec<usize> = state.selected_object_indices.clone();
                    indices.sort_unstable();
                    let mut moved: Vec<CanvasObject> = Vec::new();
                    for &idx in indices.iter().rev() {
                        if idx < state.canvas.objects.len() {
                            moved.push(state.canvas.objects.remove(idx));
                        }
                    }
                    moved.reverse();
                    let start_idx = state.canvas.objects.len();
                    let mut commands = Vec::new();
                    for obj in moved.into_iter() {
                        let new_idx = state.canvas.objects.len();
                        state.canvas.objects.push(obj);
                        commands.push(HistoryCommand::AddObject {
                            index: new_idx,
                            object: state.canvas.objects.last().unwrap().clone(),
                        });
                    }
                    state.selected_object_indices =
                        (start_idx..state.canvas.objects.len()).collect();
                    state.history.save_batch(commands);
                    state.toasts.success("对象已移至顶部!");
                }
                if ui.button("置底").clicked() {
                    // Move all selected to beginning, preserving relative order
                    let mut indices: Vec<usize> = state.selected_object_indices.clone();
                    indices.sort_unstable();
                    let mut moved: Vec<(usize, CanvasObject)> = Vec::new();
                    for &idx in indices.iter().rev() {
                        if idx < state.canvas.objects.len() {
                            moved.push((idx, state.canvas.objects.remove(idx)));
                        }
                    }
                    moved.reverse();
                    let mut commands = Vec::new();
                    for (_, obj) in &moved {
                        state.canvas.objects.insert(0, obj.clone());
                        commands.push(HistoryCommand::AddObject {
                            index: 0,
                            object: obj.clone(),
                        });
                    }
                    state.selected_object_indices = (0..moved.len()).collect();
                    state.history.save_batch(commands);
                    state.toasts.success("对象已移至底部!");
                }
                if state.selected_object_indices.len() == 1
                    && let Some(&selected_idx) = state.selected_object_indices.first()
                    && let Some(CanvasObject::Text(ref text)) =
                        state.canvas.objects.get(selected_idx).cloned()
                    && ui.button("栅格化").clicked()
                {
                    let strokes = utils::rasterize_text(text, assets::font_bytes());
                    state.canvas.objects.remove(selected_idx);
                    for stroke in strokes {
                        let stroke_obj = CanvasObject::Stroke(stroke);
                        state.canvas.objects.push(stroke_obj.clone());
                        state
                            .history
                            .save_add_object(state.canvas.objects.len() - 1, stroke_obj);
                    }
                    state
                        .history
                        .save_remove_object(selected_idx, CanvasObject::Text(text.clone()));
                    state.clear_selection();
                    state.toasts.success("已转换为笔画!");
                }
            });
        } else {
            ui.my_label(egui::RichText::new("(未选中对象)").italics());
        }
    } else if state.current_tool == CanvasTool::Brush {
        if !state.toolbar_expanded {
            return;
        }
        ui.horizontal(|ui| {
            ui.my_label("颜色:");
            let old_color = state.brush_color;
            if ui.color_edit_button_srgba(&mut state.brush_color).changed() {
                // Drain all active drawing pointers when color changes
                let drawing_ids: Vec<u64> = state
                    .pointers
                    .values()
                    .filter(|p| matches!(p.interaction, PointerInteraction::Drawing { .. }))
                    .map(|p| p.id)
                    .collect();
                for id in drawing_ids {
                    if let Some(pointer) = state.pointers.remove(&id)
                        && let PointerInteraction::Drawing { active_stroke } = pointer.interaction
                    {
                        if let StrokeWidth::Dynamic(v) = &active_stroke.width
                            && v.len() != active_stroke.points.len()
                        {
                            continue;
                        }
                        state
                            .canvas
                            .objects
                            .push(CanvasObject::Stroke(CanvasStroke {
                                points: active_stroke.points,
                                width: active_stroke.width,
                                color: old_color,
                                base_width: state.brush_width,
                                shape: None,
                            }));
                    }
                }
            }
        });

        // 颜色快捷按钮
        ui.horizontal(|ui| {
            ui.my_label("快捷颜色:");
            for color in &state.persistent.quick_colors {
                let color_name = if color.r() == 0 && color.g() == 0 && color.b() == 0 {
                    "黑"
                } else if color.r() == 255 && color.g() == 255 && color.b() == 255 {
                    "白"
                } else if color.r() == 0 && color.g() == 100 && color.b() == 255 {
                    "蓝"
                } else if color.r() == 220 && color.g() == 20 && color.b() == 60 {
                    "红"
                } else if color.r() == 34 && color.g() == 139 && color.b() == 34 {
                    "绿"
                } else if color.r() == 255 && color.g() == 140 && color.b() == 0 {
                    "橙"
                } else {
                    "自定义"
                };
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new(color_name).color(*color),
                    ))
                    .clicked()
                {
                    state.brush_color = *color;
                }
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("宽度:");
            let slider_response = ui.add(egui::Slider::new(&mut state.brush_width, 1.0..=20.0));

            // 显示大小预览
            if slider_response.dragged() || slider_response.hovered() {
                state.show_size_preview = true;
                // 使用屏幕中心位置
            } else if !slider_response.dragged() && !slider_response.hovered() {
                state.show_size_preview = false;
            }
        });

        // 画笔宽度快捷按钮
        ui.horizontal(|ui| {
            ui.my_label("快捷宽度:");
            if ui.button("小").clicked() {
                state.brush_width = 1.0;
            }
            if ui.button("中").clicked() {
                state.brush_width = 3.0;
            }
            if ui.button("大").clicked() {
                state.brush_width = 5.0;
            }
        });
    } else if state.current_tool == CanvasTool::ObjectEraser
        || state.current_tool == CanvasTool::PixelEraser
    {
        if !state.toolbar_expanded {
            return;
        }
        ui.horizontal(|ui| {
            ui.my_label("大小:");
            let slider_response = ui.add(egui::Slider::new(&mut state.eraser_size, 5.0..=50.0));

            // 显示大小预览
            if slider_response.dragged() || slider_response.hovered() {
                state.show_size_preview = true;
            } else if !slider_response.dragged() && !slider_response.hovered() {
                state.show_size_preview = false;
            }
        });

        ui.horizontal(|ui| {
            ui.my_label("清空:");
            if ui.button("OK").clicked() {
                // Save state to history before modification
                let old_objects = std::mem::take(&mut state.canvas.objects);
                state.history.save_clear_objects(old_objects);
                state.pointers.clear();
                state.clear_selection();
                state.current_tool = CanvasTool::Brush;
                if state.is_overlay_mode {
                    state.command_queue.push(AppCommand::UpdateCursorHittest);
                }
            }
        });
    } else if state.current_tool == CanvasTool::Insert {
        let prev_insert_tab = state.current_insert_tab;

        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.current_insert_tab, InsertTab::Shape, "形状");
            ui.selectable_value(&mut state.current_insert_tab, InsertTab::Text, "文本");
            if ui
                .selectable_value(&mut state.current_insert_tab, InsertTab::Image, "图片")
                .clicked()
            {
                let path = rfd::FileDialog::new()
                    .add_filter("图片", IMAGE_FILE_EXTS)
                    .pick_file();
                state.current_insert_tab = prev_insert_tab;
                if let Some(path) = path {
                    match image::open(path) {
                        Ok(img) => {
                            const MAX_TEXTURE_SIZE: u32 = 2048;

                            let img = if img.width() > MAX_TEXTURE_SIZE
                                || img.height() > MAX_TEXTURE_SIZE
                            {
                                utils::resize_image_for_texture(img, MAX_TEXTURE_SIZE)
                            } else {
                                img
                            };

                            let img_rgba = img.to_rgba8();
                            let (width, height) = img_rgba.dimensions();
                            let aspect_ratio = width as f32 / height as f32;

                            let target_width = 300.0_f32;
                            let target_height = target_width / aspect_ratio;

                            let ctx = ui.ctx();
                            let texture = ctx.load_texture(
                                "inserted_image",
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [width as usize, height as usize],
                                    &img_rgba,
                                ),
                                egui::TextureOptions::LINEAR,
                            );

                            let image_data: Arc<[u8]> = img_rgba.into_raw().into();
                            let new_image = CanvasImage {
                                texture,
                                pos: Pos2::new(100.0, 100.0),
                                size: egui::vec2(target_width, target_height),
                                aspect_ratio,
                                marked_for_deletion: false,

                                image_data,
                                image_size: [width, height],
                            };
                            let index = state.canvas.objects.len();
                            state
                                .history
                                .save_add_object(index, CanvasObject::Image(new_image.clone()));
                            state.canvas.objects.push(CanvasObject::Image(new_image));

                            state.current_tool = CanvasTool::Select;
                            if state.is_overlay_mode {
                                state.command_queue.push(AppCommand::UpdateCursorHittest);
                            }
                        }
                        Err(err) => {
                            state.toasts.error(format!("图片插入失败: {}!", err));
                        }
                    }
                } else {
                    state.toasts.error("图片插入失败: 已取消!");
                }
            }
        });

        match state.current_insert_tab {
            InsertTab::Shape => {
                ui.my_label("形状类型:");

                ui.horizontal(|ui| {
                    let prev = state.selected_shape_type;

                    if ui
                        .selectable_value(
                            &mut state.selected_shape_type,
                            Some(CanvasShapeType::Line),
                            "线",
                        )
                        .clicked()
                        && prev == Some(CanvasShapeType::Line)
                    {
                        state.selected_shape_type = None;
                    }
                    if ui
                        .selectable_value(
                            &mut state.selected_shape_type,
                            Some(CanvasShapeType::Arrow),
                            "箭头",
                        )
                        .clicked()
                        && prev == Some(CanvasShapeType::Arrow)
                    {
                        state.selected_shape_type = None;
                    }
                    if ui
                        .selectable_value(
                            &mut state.selected_shape_type,
                            Some(CanvasShapeType::Rectangle),
                            "矩形",
                        )
                        .clicked()
                        && prev == Some(CanvasShapeType::Rectangle)
                    {
                        state.selected_shape_type = None;
                    }
                    if ui
                        .selectable_value(
                            &mut state.selected_shape_type,
                            Some(CanvasShapeType::Triangle),
                            "三角形",
                        )
                        .clicked()
                        && prev == Some(CanvasShapeType::Triangle)
                    {
                        state.selected_shape_type = None;
                    }
                    if ui
                        .selectable_value(
                            &mut state.selected_shape_type,
                            Some(CanvasShapeType::Circle),
                            "圆形",
                        )
                        .clicked()
                        && prev == Some(CanvasShapeType::Circle)
                    {
                        state.selected_shape_type = None;
                    }

                    if prev != state.selected_shape_type {
                        state.shapes_inserted_count = 0;
                    }
                });

                if state.selected_shape_type.is_some() {
                    ui.my_label(egui::RichText::new("(在画布上滑动以绘制形状)").italics());
                }

                ui.checkbox(&mut state.continuous_insert, "连续插入");
            }
            InsertTab::Text => {
                ui.horizontal(|ui| {
                    ui.my_label("文本内容:");
                    ui.text_edit_singleline(&mut state.new_text_content);
                });

                ui.horizontal(|ui| {
                    if ui.button("确认").clicked() {
                        let text_size = ui
                            .painter()
                            .layout_no_wrap(
                                state.new_text_content.clone(),
                                egui::FontId::proportional(16.0),
                                Color32::WHITE,
                            )
                            .size();
                        let new_text = CanvasText {
                            text: state.new_text_content.clone(),
                            pos: Pos2::new(100.0, 100.0),
                            color: Color32::WHITE,
                            font_size: 16.0,

                            cached_size: Some(text_size),
                        };
                        let index = state.canvas.objects.len();
                        state
                            .history
                            .save_add_object(index, CanvasObject::Text(new_text.clone()));
                        state.canvas.objects.push(CanvasObject::Text(new_text));
                        state.current_tool = CanvasTool::Select;
                        if state.is_overlay_mode {
                            state.command_queue.push(AppCommand::UpdateCursorHittest);
                        }
                        state.new_text_content.clear();
                    }

                    if ui.button("取消").clicked() {
                        state.new_text_content.clear();
                    }
                });
            }
            InsertTab::Image => {}
        }
    } else if state.current_tool == CanvasTool::Settings {
        ui_toolbar_settings(state, ctx, ui, window);
    }
}

fn tool_button(state: &mut AppState, ui: &mut Ui, tool: CanvasTool, label: &str) -> bool {
    let resp = ui.selectable_value(&mut state.current_tool, tool, label);
    if resp.clicked() && !resp.changed() {
        state.toolbar_expanded = !state.toolbar_expanded;
    }
    resp.changed()
}

fn ui_toolbar_tools_selector(state: &mut AppState, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.my_label("工具:");
        // TODO: egui doesn't support rendering fonts with colors
        let old_tool = state.current_tool;
        if ((state.is_overlay_mode
            && ui
                .selectable_value(&mut state.current_tool, CanvasTool::Passthrough, "穿透")
                .changed())
            || ui
                .selectable_value(&mut state.current_tool, CanvasTool::Select, "选择")
                .changed()
            || ui
                .selectable_value(&mut state.current_tool, CanvasTool::View, "视图")
                .changed()
            || tool_button(state, ui, CanvasTool::Brush, "画笔")
            || tool_button(state, ui, CanvasTool::ObjectEraser, "对象擦")
            || tool_button(state, ui, CanvasTool::PixelEraser, "像素擦")
            || ui
                .selectable_value(&mut state.current_tool, CanvasTool::Insert, "插入")
                .changed()
            || ui
                .selectable_value(&mut state.current_tool, CanvasTool::Settings, "设置")
                .changed())
            && state.current_tool != old_tool
        {
            clear_interaction_state(state);
            if state.is_overlay_mode {
                state.command_queue.push(AppCommand::UpdateCursorHittest);
            }
            state.toolbar_expanded = false;
        }
    });
}

pub fn ui_toolbar(
    state: &mut AppState,
    ctx: &Context,
    window: &Arc<Window>,
    is_helper: bool,
) -> Option<egui::Rect> {
    let content_rect = ctx.content_rect();
    let mut w = egui::Window::new("工具栏")
        .resizable(false)
        .collapsible(false)
        .enabled(!state.show_welcome_window)
        .title_bar(false);

    if is_helper {
        w = w.movable(false).constrain(false).fixed_pos([10.0, 10.0]);
    } else {
        w = w
            .movable(true)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .default_pos([content_rect.center().x, content_rect.max.y - 20.0]);
    }

    let inner_response = w.show(ctx, |ui| {
        ui_toolbar_tools_content(state, ctx, ui, window);

        let show_sep = !matches!(
            state.current_tool,
            CanvasTool::Brush | CanvasTool::ObjectEraser | CanvasTool::PixelEraser
        ) || state.toolbar_expanded;
        if show_sep {
            ui.separator();
        }

        ui_toolbar_tools_selector(state, ui);

        ui.separator();

        ui_history(state, ui);

        ui.separator();

        ui_window_controls(state, ui, window);
    });

    inner_response.map(|ir| ir.response.rect)
}

#[cfg_attr(feature = "profiling", profiling::function)]
pub fn ui_canvas(state: &mut AppState, ctx: &Context) {
    let id = Id::new((ctx.viewport_id(), "central_panel"));
    let mut panel_ui = Ui::new(
        ctx.clone(),
        id,
        UiBuilder::new()
            .layer_id(LayerId::background())
            .max_rect(ctx.content_rect()),
    );
    panel_ui.set_clip_rect(ctx.content_rect());

    CentralPanel::default().show_inside(&mut panel_ui, |ui| {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size(),
            if !state.persistent.low_latency_mode {
                Sense::click_and_drag()
            } else {
                Sense::drag()
            },
        );

        let painter = ui.painter();
        let content_rect = ui.ctx().content_rect();
        let view_offset = state.view_offset;
        let zoom = state.view_zoom;
        let screen_center = content_rect.center();

        // 绘制所有对象 (带视图裁剪)
        for (i, object) in state.canvas.objects.iter().enumerate() {
            let selected = state.is_selected(i);
            // 对象包围盒完全在视图外则跳过
            let obj_bbox = object.bounding_box();
            let screen_min = (obj_bbox.min - view_offset) * zoom;
            let screen_max = (obj_bbox.max - view_offset) * zoom;
            let screen_bbox = egui::Rect::from_min_max(screen_min, screen_max);
            if screen_bbox.intersects(content_rect) {
                object.paint(painter, selected, view_offset, zoom);
            }
        }

        // 绘制当前正在绘制的笔画
        for pointer in state.pointers.values() {
            if let PointerInteraction::Drawing { active_stroke } = &pointer.interaction {
                if let StrokeWidth::Dynamic(v) = &active_stroke.width
                    && v.len() != active_stroke.points.len()
                {
                    continue;
                }
                let color = state.brush_color;
                let offset_points: Vec<Pos2> = active_stroke
                    .points
                    .iter()
                    .map(|p| (*p - view_offset) * zoom)
                    .collect();

                let z_width_first = active_stroke.width.first() * zoom / 2.0;

                painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                    offset_points[0],
                    z_width_first,
                    color,
                )));
                if active_stroke.points.len() >= 2 {
                    let z_width_last = active_stroke.width.last() * zoom / 2.0;
                    painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                        offset_points[offset_points.len() - 1],
                        z_width_last,
                        color,
                    )));
                    match &active_stroke.width {
                        StrokeWidth::Fixed(w) => {
                            if active_stroke.points.len() == 2 {
                                painter.line_segment(
                                    [offset_points[0], offset_points[1]],
                                    Stroke::new(*w * zoom, color),
                                );
                            } else {
                                let path = egui::epaint::PathShape::line(
                                    offset_points.clone(),
                                    Stroke::new(*w * zoom, color),
                                );
                                painter.add(egui::Shape::Path(path));
                            }
                        }
                        StrokeWidth::Dynamic(widths) => {
                            for i in 0..offset_points.len() - 1 {
                                let avg_width = (widths[i] + widths[i + 1]) / 2.0 * zoom;
                                painter.line_segment(
                                    [offset_points[i], offset_points[i + 1]],
                                    Stroke::new(avg_width, color),
                                );
                            }
                        }
                    }
                }
            }
        }

        // 绘制正在拖拽插入的形状预览
        for pointer in state.pointers.values() {
            if let PointerInteraction::ShapeInsert {
                start_pos,
                shape_type,
            } = &pointer.interaction
            {
                let sp = (*start_pos - view_offset) * zoom;
                let ep = (pointer.pos - view_offset) * zoom;
                let z_stroke = Stroke::new(3.0_f32 * zoom, Color32::WHITE);
                match shape_type {
                    CanvasShapeType::Line => {
                        painter.line_segment([sp, ep], z_stroke);
                        painter.circle_filled(sp, 1.5_f32 * zoom, Color32::WHITE);
                        painter.circle_filled(ep, 1.5_f32 * zoom, Color32::WHITE);
                    }
                    CanvasShapeType::Arrow => {
                        let len = sp.distance(ep);
                        if len > 1.0 * zoom {
                            let dir = (ep - sp) / len;
                            let arrow_size = (len * 0.15).max(10.0 * zoom);
                            let angle = 30.0_f32.to_radians();
                            let cos = angle.cos();
                            let sin = angle.sin();
                            let left_dir =
                                egui::vec2(dir.x * cos - dir.y * sin, dir.x * sin + dir.y * cos);
                            let right_dir =
                                egui::vec2(dir.x * cos + dir.y * sin, -dir.x * sin + dir.y * cos);
                            painter.line_segment([sp, ep], z_stroke);
                            painter.line_segment([ep, ep - left_dir * arrow_size], z_stroke);
                            painter.line_segment([ep, ep - right_dir * arrow_size], z_stroke);
                        }
                    }
                    CanvasShapeType::Rectangle => {
                        let rect = Rect::from_two_pos(sp, ep);
                        painter.rect_stroke(rect, 0.0, z_stroke, egui::StrokeKind::Outside);
                    }
                    CanvasShapeType::Triangle => {
                        let rect = Rect::from_two_pos(sp, ep);
                        let size = rect.width().max(rect.height());
                        let top_left = rect.min;
                        let p1 = top_left + egui::vec2(size / 2.0, 0.0);
                        let p2 = top_left + egui::vec2(0.0, size);
                        let p3 = top_left + egui::vec2(size, size);
                        painter.add(egui::Shape::convex_polygon(
                            vec![p1, p2, p3],
                            Color32::TRANSPARENT,
                            z_stroke,
                        ));
                    }
                    CanvasShapeType::Circle => {
                        let center = sp + (ep - sp) / 2.0;
                        let radius = sp.distance(ep) / 2.0;
                        painter.circle_stroke(center, radius, z_stroke);
                    }
                }
            }
        }

        // 绘制大小预览圆圈
        if state.show_size_preview {
            let content_rect = ui.ctx().content_rect();
            let pos = content_rect.center();
            let preview_size = match state.current_tool {
                CanvasTool::Brush => state.brush_width * zoom,
                CanvasTool::ObjectEraser | CanvasTool::PixelEraser => state.eraser_size * zoom,
                _ => unreachable!(),
            };
            utils::draw_size_preview(painter, pos, preview_size);
        }
        // 绘制多选框 (marquee)
        for pointer in state.pointers.values() {
            if let PointerInteraction::MarqueeSelect { points, .. } = &pointer.interaction {
                if points.len() < 2 {
                    continue;
                }
                // Convert canvas-coord points to screen coords
                let screen_pts: Vec<Pos2> =
                    points.iter().map(|p| (*p - view_offset) * zoom).collect();

                // Draw filled polygon via triangulation
                if screen_pts.len() >= 3 {
                    let triangles = utils::triangulate_polygon(&screen_pts);
                    for tri in &triangles {
                        painter.add(egui::Shape::convex_polygon(
                            tri.to_vec(),
                            Color32::from_black_alpha(30),
                            Stroke::NONE,
                        ));
                    }
                }

                // Draw polygon outline
                if screen_pts.len() >= 2 {
                    let mut shape_points = screen_pts.clone();
                    if screen_pts.len() >= 3 {
                        shape_points.push(screen_pts[0]); // close the polygon
                    }
                    painter.add(egui::Shape::line(
                        shape_points,
                        Stroke::new(1.5 * zoom, Color32::WHITE),
                    ));
                }
            }
        }

        // 绘制触控点
        if state.show_touch_points {
            for pointer in state.pointers.values() {
                if pointer.id == 0 {
                    continue;
                }
                let pos = (pointer.pos - view_offset) * zoom;
                painter.circle_filled(
                    pos,
                    15.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 180),
                );
                painter.circle_stroke(pos, 15.0, Stroke::new(2.0_f32, Color32::BLUE));

                // 绘制触控 ID
                let text_galley = painter.layout_no_wrap(
                    format!("{}", pointer.id),
                    egui::FontId::proportional(14.0),
                    Color32::BLACK,
                );
                let text_pos = Pos2::new(
                    pos.x - text_galley.size().x / 2.0,
                    pos.y - text_galley.size().y / 2.0,
                );
                let text_shape = egui::epaint::TextShape {
                    pos: text_pos,
                    galley: text_galley,
                    underline: egui::Stroke::NONE,
                    override_text_color: None,
                    angle: 0.0,
                    fallback_color: Color32::BLACK,
                    opacity_factor: 1.0,
                };
                painter.add(text_shape);
            }
        }

        // when mouse passthrough tool is selected, skip canvas interaction
        if state.is_overlay_mode && state.current_tool == CanvasTool::Passthrough {
            return;
        }

        // 处理指针输入
        let has_touch = state.pointers.keys().any(|&k| k != 0);
        let pointer_pos = if has_touch {
            None
        } else {
            response.interact_pointer_pos()
        };
        // 屏幕坐标 -> 画布坐标
        let canvas_pos = pointer_pos.map(|p| p / zoom + view_offset);

        match state.current_tool {
            CanvasTool::Settings | CanvasTool::Passthrough => {}

            CanvasTool::View => {
                // Mouse wheel zoom
                if !has_touch && response.hovered() {
                    let scroll = ui.ctx().input(|i| i.smooth_scroll_delta);
                    if scroll.y != 0.0 {
                        let new_zoom = (state.view_zoom
                            * (1.0 + crate::state::AppState::ZOOM_STEP).powf(scroll.y))
                        .clamp(
                            crate::state::AppState::MIN_ZOOM,
                            crate::state::AppState::MAX_ZOOM,
                        );
                        // Zoom around cursor (or screen center if cursor not available)
                        let s = pointer_pos.unwrap_or(screen_center);
                        state.view_offset += s.to_vec2() * (1.0 / state.view_zoom - 1.0 / new_zoom);
                        state.view_zoom = new_zoom;
                    }
                }
                if !has_touch {
                    if response.drag_started() {
                        if let Some(screen_pos) = pointer_pos
                            && let Some(pos) = canvas_pos
                        {
                            state.pointers.insert(
                                0,
                                PointerState {
                                    id: 0,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::Panning {
                                        last_pos: screen_pos,
                                    },
                                },
                            );
                        }
                    } else if response.dragged() {
                        if let Some(pointer) = state.pointers.get_mut(&0)
                            && matches!(pointer.interaction, PointerInteraction::Panning { .. })
                            && let Some(screen_pos) = pointer_pos
                            && let Some(pos) = canvas_pos
                        {
                            if let PointerInteraction::Panning { ref mut last_pos } =
                                pointer.interaction
                            {
                                let delta = screen_pos - *last_pos;
                                state.view_offset -= delta / zoom;
                                *last_pos = screen_pos;
                            }
                            pointer.pos = pos;
                        }
                    } else if response.drag_stopped() {
                        state.pointers.remove(&0);
                    }
                }
            }

            CanvasTool::Insert => {
                if state.current_insert_tab == InsertTab::Shape
                    && state.selected_shape_type.is_some()
                    && !has_touch
                {
                    if response.drag_started() {
                        if let Some(pos) = canvas_pos
                            && let Some(screen_pos) = pointer_pos
                            && screen_pos.x >= rect.min.x
                            && screen_pos.x <= rect.max.x
                            && screen_pos.y >= rect.min.y
                            && screen_pos.y <= rect.max.y
                            && let Some(shape_type) = state.selected_shape_type
                        {
                            state.pointers.insert(
                                0,
                                PointerState {
                                    id: 0,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::ShapeInsert {
                                        start_pos: pos,
                                        shape_type,
                                    },
                                },
                            );
                        }
                    } else if response.dragged() {
                        if let Some(pointer) = state.pointers.get_mut(&0)
                            && matches!(pointer.interaction, PointerInteraction::ShapeInsert { .. })
                            && let Some(pos) = canvas_pos
                        {
                            pointer.prev_pos = Some(pointer.pos);
                            pointer.pos = pos;
                        }
                    } else if response.drag_stopped()
                        && let Some(pointer) = state.pointers.remove(&0)
                        && let PointerInteraction::ShapeInsert {
                            start_pos,
                            shape_type,
                        } = pointer.interaction
                    {
                        let end_pos = pointer.pos;
                        utils::ui::create_shape_object(state, shape_type, start_pos, end_pos);
                    }
                }
            }

            CanvasTool::Select => {
                if !has_touch {
                    // Handle click: support shift for toggle, single select without shift
                    if response.clicked()
                        && let Some(click_pos) = canvas_pos
                    {
                        let shift = ui.ctx().input(|i| i.modifiers.shift);
                        if !shift {
                            state.clear_selection();
                        }

                        if state.persistent.click_or_drag_to_single_select {
                            for (i, object) in state.canvas.objects.iter().enumerate().rev() {
                                if object.bounding_box().contains(click_pos) {
                                    if shift {
                                        state.toggle_selection(i);
                                    } else {
                                        state.selected_object_indices.push(i);
                                    }
                                    break;
                                }
                            }
                        }
                    }

                    // Handle drag start: Selecting on object or MarqueeSelect on empty space
                    if response.drag_started()
                        && let Some(pos) = canvas_pos
                    {
                        // Hit-test: find object under cursor (last-to-first) for dragging
                        let hit_idx = state
                            .canvas
                            .objects
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, obj)| obj.bounding_box().contains(pos))
                            .map(|(i, _)| i);

                        if let Some(hit) = hit_idx
                            && (state.persistent.click_or_drag_to_single_select
                                || state.is_selected(hit))
                        {
                            // Dragging on an object → entering selecting/move/resize interaction
                            if !state.is_selected(hit) {
                                // Dragging on an unselected object: toggle in, or single-select
                                let shift = ui.ctx().input(|i| i.modifiers.shift);
                                if shift {
                                    state.toggle_selection(hit);
                                } else {
                                    state.clear_selection();
                                    state.selected_object_indices.push(hit);
                                }
                            }
                            let (dragged_handle, drag_original_transforms) =
                                if let Some(&primary_idx) = state.selected_object_indices.first()
                                    && primary_idx < state.canvas.objects.len()
                                {
                                    let object = &state.canvas.objects[primary_idx];
                                    let bbox = object.bounding_box();
                                    let handle = if state.selected_object_indices.len() == 1 {
                                        utils::get_transform_handle_at_pos(bbox, pos)
                                    } else {
                                        None
                                    };
                                    let transforms: Vec<(usize, ObjectTransform)> = state
                                        .selected_object_indices
                                        .iter()
                                        .filter_map(|&i| {
                                            if i < state.canvas.objects.len() {
                                                Some((i, state.canvas.objects[i].get_transform()))
                                            } else {
                                                None
                                            }
                                        })
                                        .collect();
                                    (handle, transforms)
                                } else {
                                    (None, Vec::new())
                                };
                            state.pointers.insert(
                                0,
                                PointerState {
                                    id: 0,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::Selecting {
                                        drag_start: pos,
                                        dragged_handle,
                                        drag_original_transforms,
                                        drag_accumulated_delta: egui::Vec2::ZERO,
                                    },
                                },
                            );
                        } else {
                            // Dragging on empty space → marquee select
                            state.pointers.insert(
                                0,
                                PointerState {
                                    id: 0,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::MarqueeSelect {
                                        drag_start: pos,
                                        points: vec![pos],
                                    },
                                },
                            );
                        }
                    }

                    // Handle dragging: move/resize selected objects or marquee
                    if response.dragged()
                        && let Some(current_pos) = canvas_pos
                        && let Some(pointer) = state.pointers.get_mut(&0)
                    {
                        pointer.pos = current_pos;
                        match &mut pointer.interaction {
                            PointerInteraction::Selecting {
                                drag_start,
                                dragged_handle,
                                drag_accumulated_delta,
                                ..
                            } => {
                                let delta = current_pos - *drag_start;

                                if let Some(handle) = dragged_handle {
                                    // Resize only on the primary (first) selected object
                                    if let Some(&primary_idx) =
                                        state.selected_object_indices.first()
                                        && primary_idx < state.canvas.objects.len()
                                        && let Some(object) =
                                            state.canvas.objects.get_mut(primary_idx)
                                    {
                                        object.transform(*handle, delta, *drag_start, current_pos);
                                    }
                                } else {
                                    // Move all selected objects by delta
                                    for &idx in &state.selected_object_indices.clone() {
                                        if idx < state.canvas.objects.len()
                                            && let Some(object) = state.canvas.objects.get_mut(idx)
                                        {
                                            CanvasObject::move_object(object, delta);
                                        }
                                    }
                                    *drag_accumulated_delta += delta;
                                }

                                *drag_start = current_pos;
                            }
                            PointerInteraction::MarqueeSelect { points, .. } => {
                                // Collect points for lasso polygon
                                let min_dist = 2.0; // minimum distance in canvas coords
                                if points
                                    .last()
                                    .is_none_or(|last| last.distance(current_pos) >= min_dist)
                                {
                                    points.push(current_pos);
                                }
                            }
                            _ => {}
                        }
                    }

                    // Handle drag stop: save move/resize to history, or select with marquee
                    if response.drag_stopped()
                        && let Some(pointer) = state.pointers.remove(&0)
                    {
                        match pointer.interaction {
                            PointerInteraction::Selecting {
                                drag_accumulated_delta,
                                drag_original_transforms,
                                dragged_handle,
                                ..
                            } => {
                                if dragged_handle.is_some() {
                                    // Resize: save TransformObject for single object
                                    if let Some(&(sel_idx, _)) = drag_original_transforms.first()
                                        && sel_idx < state.canvas.objects.len()
                                        && let Some(object) = state.canvas.objects.get(sel_idx)
                                    {
                                        let new_transform = object.get_transform();
                                        let old_transform = drag_original_transforms[0].1.clone();
                                        state.history.save_transform_object(
                                            sel_idx,
                                            old_transform,
                                            new_transform,
                                        );
                                    }
                                } else if drag_accumulated_delta != egui::Vec2::ZERO {
                                    // Move: save MoveObject(s) for all selected objects
                                    let commands: Vec<HistoryCommand> = drag_original_transforms
                                        .iter()
                                        .map(|&(idx, _)| HistoryCommand::MoveObject {
                                            index: idx,
                                            old_position: -drag_accumulated_delta,
                                            new_position: drag_accumulated_delta,
                                        })
                                        .collect();
                                    if commands.len() <= 1 {
                                        if let Some(cmd) = commands.into_iter().next() {
                                            state.history.push_command(cmd);
                                        }
                                    } else {
                                        state.history.save_batch(commands);
                                    }
                                }
                            }
                            PointerInteraction::MarqueeSelect {
                                drag_start: _,
                                mut points,
                            } => {
                                // Close the polygon (auto-connect start to end)
                                if points.len() >= 2
                                    && points.first().unwrap().distance(*points.last().unwrap())
                                        > 0.5
                                {
                                    points.push(*points.first().unwrap());
                                }

                                let shift = ui.ctx().input(|i| i.modifiers.shift);
                                if !shift {
                                    state.clear_selection();
                                }

                                if points.len() >= 3 {
                                    // Simplify polygon for hit-testing performance
                                    let simplified = utils::simplify_polygon(&points, 4.0);

                                    // Collect matching objects based on mode
                                    let mode = state.marquee_match_mode;
                                    let intersecting: Vec<usize> = state
                                        .canvas
                                        .objects
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, obj)| {
                                            if let CanvasObject::Stroke(s) = obj {
                                                match mode {
                                                    MarqueeMatchMode::Overlapping => {
                                                        s.points.iter().any(|p| {
                                                            utils::point_in_polygon(*p, &simplified)
                                                        })
                                                    }
                                                    MarqueeMatchMode::Containing => {
                                                        s.points.iter().all(|p| {
                                                            utils::point_in_polygon(*p, &simplified)
                                                        })
                                                    }
                                                }
                                            } else {
                                                match mode {
                                                    MarqueeMatchMode::Overlapping => {
                                                        utils::polygon_intersects_rect(
                                                            &simplified,
                                                            obj.bounding_box(),
                                                        )
                                                    }
                                                    MarqueeMatchMode::Containing => {
                                                        utils::polygon_contains_rect(
                                                            &simplified,
                                                            obj.bounding_box(),
                                                        )
                                                    }
                                                }
                                            }
                                        })
                                        .map(|(i, _)| i)
                                        .collect();
                                    for i in intersecting {
                                        if shift {
                                            state.toggle_selection(i);
                                        } else {
                                            state.selected_object_indices.push(i);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            CanvasTool::ObjectEraser => {
                let eraser_positions: Vec<Pos2> = if has_touch {
                    state
                        .pointers
                        .values()
                        .filter(|p| matches!(p.interaction, PointerInteraction::Erasing))
                        .map(|p| p.pos)
                        .collect()
                } else if response.drag_started() || response.clicked() || response.dragged() {
                    canvas_pos.into_iter().collect()
                } else {
                    vec![]
                };

                for pos in eraser_positions {
                    utils::draw_size_preview(
                        painter,
                        (pos - view_offset) * zoom,
                        state.eraser_size * zoom,
                    );

                    let mut to_remove = Vec::new();
                    for (i, object) in state.canvas.objects.iter().enumerate().rev() {
                        match object {
                            CanvasObject::Image(img) => {
                                let img_rect = egui::Rect::from_min_size(img.pos, img.size);
                                if img_rect.contains(pos) {
                                    to_remove.push(i);
                                }
                            }
                            CanvasObject::Text(text) => {
                                if text.bounding_box().contains(pos) {
                                    to_remove.push(i);
                                }
                            }
                            CanvasObject::Shape(shape) => {
                                let shape_rect = shape.bounding_box();
                                if shape_rect.contains(pos) {
                                    to_remove.push(i);
                                }
                            }
                            CanvasObject::Stroke(stroke) => {
                                if utils::point_intersects_stroke(pos, stroke, state.eraser_size) {
                                    to_remove.push(i);
                                }
                            }
                        }
                    }
                    for i in to_remove {
                        let object = state.canvas.objects.remove(i);
                        state.history.save_remove_object(i, object);
                    }
                }
            }

            CanvasTool::PixelEraser => {
                if !has_touch {
                    if response.drag_started() || response.clicked() {
                        if let Some(pos) = canvas_pos {
                            state.pointers.insert(
                                0,
                                PointerState {
                                    id: 0,
                                    pos,
                                    prev_pos: None,
                                    interaction: PointerInteraction::Erasing,
                                },
                            );
                        }
                    } else if response.dragged() {
                        if let Some(pointer) = state.pointers.get_mut(&0)
                            && matches!(pointer.interaction, PointerInteraction::Erasing)
                            && let Some(pos) = canvas_pos
                        {
                            pointer.pos = pos;
                        }
                    } else {
                        state.pointers.remove(&0);
                    }
                }

                let eraser_positions: Vec<Pos2> = {
                    let mut positions = Vec::new();
                    for pointer in state.pointers.values() {
                        if !matches!(pointer.interaction, PointerInteraction::Erasing) {
                            continue;
                        }
                        if let Some(prev) = pointer.prev_pos {
                            let dist = prev.distance(pointer.pos);
                            let step = state.eraser_size * 0.5;
                            if dist > step {
                                let num_steps = (dist / step).ceil() as usize;
                                for j in 1..num_steps {
                                    let t = j as f32 / num_steps as f32;
                                    positions.push(prev.lerp(pointer.pos, t));
                                }
                            }
                        }
                        positions.push(pointer.pos);
                    }
                    positions
                };

                for pointer in state.pointers.values() {
                    if matches!(pointer.interaction, PointerInteraction::Erasing) {
                        utils::draw_size_preview(
                            painter,
                            (pointer.pos - view_offset) * zoom,
                            state.eraser_size * zoom,
                        );
                    }
                }

                for pos in eraser_positions {
                    let eraser_radius = state.eraser_size / 2.0;
                    let eraser_rect = egui::Rect::from_center_size(
                        pos,
                        egui::vec2(state.eraser_size, state.eraser_size),
                    );

                    let mut new_strokes = Vec::new();
                    let mut strokes_modified = false;

                    for object in &state.canvas.objects {
                        if let CanvasObject::Stroke(stroke) = object {
                            if stroke.points.len() < 2 {
                                let single_point = stroke.points[0];
                                let dist = pos.distance(single_point);
                                if dist > eraser_radius + stroke.width.first() / 2.0 {
                                    new_strokes.push(stroke.clone());
                                }
                                strokes_modified = true;
                                continue;
                            }

                            if !stroke.bounding_box().intersects(eraser_rect) {
                                new_strokes.push(stroke.clone());
                                continue;
                            }

                            strokes_modified = true;

                            // Interpolate shape strokes for finer eraser granularity
                            let (points, widths) = if stroke.shape.is_some() {
                                const STEP: f32 = 5.0;
                                let w = stroke.width.first();

                                let estimated = stroke.points.len() * 4;
                                let mut new_pts = Vec::with_capacity(estimated);
                                let mut new_w = Vec::with_capacity(estimated);

                                for i in 0..stroke.points.len() - 1 {
                                    let p1 = stroke.points[i];
                                    let p2 = stroke.points[i + 1];
                                    let dist = p1.distance(p2);
                                    if dist > STEP {
                                        let num = (dist / STEP).ceil() as usize;
                                        for j in 0..num {
                                            let t = j as f32 / num as f32;
                                            new_pts.push(p1.lerp(p2, t));
                                            new_w.push(w);
                                        }
                                    } else {
                                        new_pts.push(p1);
                                        new_w.push(w);
                                    }
                                }
                                if let Some(&last) = stroke.points.last() {
                                    new_pts.push(last);
                                    new_w.push(stroke.width.last());
                                }
                                (new_pts, StrokeWidth::Dynamic(new_w))
                            } else {
                                (stroke.points.clone(), stroke.width.clone())
                            };

                            let mut current_points = Vec::new();
                            let mut current_widths = Vec::new();

                            current_points.push(points[0]);
                            current_widths.push(widths.first());

                            for i in 0..points.len() - 1 {
                                let p1 = points[i];
                                let p2 = points[i + 1];
                                let segment_width = widths.get(i);

                                let dist = utils::point_to_line_segment_distance(pos, p1, p2);

                                if dist > eraser_radius + segment_width / 2.0 {
                                    current_points.push(p2);
                                    current_widths.push(widths.get(i + 1));
                                } else {
                                    if current_points.len() >= 2 {
                                        new_strokes.push(CanvasStroke {
                                            points: current_points.clone(),
                                            width: current_widths.clone().into(),
                                            color: stroke.color,
                                            base_width: stroke.base_width,
                                            shape: None,
                                        });
                                    }
                                    current_points = vec![p2];
                                    current_widths = vec![widths.get(i + 1)];
                                }
                            }

                            if current_points.len() >= 2 {
                                new_strokes.push(CanvasStroke {
                                    points: current_points,
                                    width: current_widths.into(),
                                    color: stroke.color,
                                    base_width: stroke.base_width,
                                    shape: None,
                                });
                            }
                        }
                    }

                    if strokes_modified {
                        let original_stroke_count = state
                            .canvas
                            .objects
                            .iter()
                            .filter(|obj| matches!(obj, CanvasObject::Stroke(_)))
                            .count();
                        let new_stroke_count = new_strokes.len();
                        if original_stroke_count != new_stroke_count {
                            let non_strokes: Vec<_> = state
                                .canvas
                                .objects
                                .iter()
                                .filter(|obj| !matches!(obj, CanvasObject::Stroke(_)))
                                .cloned()
                                .collect();
                            let old_objects = std::mem::take(&mut state.canvas.objects);
                            state.history.save_clear_objects(old_objects);
                            state.canvas.objects = non_strokes;
                        } else {
                            state
                                .canvas
                                .objects
                                .retain(|obj| !matches!(obj, CanvasObject::Stroke(_)));
                        }

                        for stroke in new_strokes {
                            state.canvas.objects.push(CanvasObject::Stroke(stroke));
                        }
                    }
                }

                for pointer in state.pointers.values_mut() {
                    if matches!(pointer.interaction, PointerInteraction::Erasing) {
                        pointer.prev_pos = Some(pointer.pos);
                    }
                }
            }

            CanvasTool::Brush => {
                // Skip mouse handling if touch is active
                if has_touch {
                    return;
                }

                let is_drawing = state
                    .pointers
                    .get(&0)
                    .is_some_and(|p| matches!(p.interaction, PointerInteraction::Drawing { .. }));

                // 画笔工具
                if response.drag_started() {
                    if let Some(screen_pos) = pointer_pos
                        && screen_pos.x >= rect.min.x
                        && screen_pos.x <= rect.max.x
                        && screen_pos.y >= rect.min.y
                        && screen_pos.y <= rect.max.y
                        && let Some(pos) = canvas_pos
                    {
                        brush_stroke_start(state, 0, pos);
                    }
                } else if response.dragged() {
                    if is_drawing && let Some(pos) = canvas_pos {
                        brush_stroke_add_point(state, 0, pos, false);
                    }
                } else if response.drag_stopped() {
                    if is_drawing {
                        brush_stroke_end(state, 0);
                    }
                } else if response.clicked() {
                    // 处理单击事件 - 绘制单个点
                    if let Some(screen_pos) = pointer_pos
                        && screen_pos.x >= rect.min.x
                        && screen_pos.x <= rect.max.x
                        && screen_pos.y >= rect.min.y
                        && screen_pos.y <= rect.max.y
                        && let Some(pos) = canvas_pos
                    {
                        let new_stroke = CanvasStroke {
                            points: vec![pos],
                            width: StrokeWidth::Fixed(state.brush_width),
                            color: state.brush_color,
                            base_width: state.brush_width,
                            shape: None,
                        };
                        let index = state.canvas.objects.len();
                        state
                            .history
                            .save_add_object(index, CanvasObject::Stroke(new_stroke.clone()));
                        state.canvas.objects.push(CanvasObject::Stroke(new_stroke));
                    }
                }

                // 如果鼠标在画布内移动且正在绘制，也添加点（用于平滑绘制）
                if response.hovered()
                    && is_drawing
                    && let Some(pos) = canvas_pos
                {
                    brush_stroke_add_point(state, 0, pos, true);
                }
            }
        }
    });
}

const IMAGE_FILE_EXTS: &[&str; 6] = &["png", "jpg", "jpeg", "bmp", "webp", "ico"];
