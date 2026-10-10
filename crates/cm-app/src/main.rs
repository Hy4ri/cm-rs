mod app;
mod cli;
mod text_input;
mod theme;
mod widgets;

use gpui::{
    App, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, actions, prelude::*, px,
    size,
};

actions!(cm, [Quit]);

fn main() {
    // `cm-rs <command> ...` runs the command-line tools instead of the GUI.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        std::process::exit(cli::run(&args));
    }

    Application::new().run(|cx: &mut App| {
        text_input::bind_keys(cx);
        cx.bind_keys([KeyBinding::new("secondary-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1360.), px(860.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("Content Manager (Rust)".into()), ..Default::default() }),
                app_id: Some("cm-rs".into()),
                ..Default::default()
            },
            |_, cx| cx.new(app::CmApp::new),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
