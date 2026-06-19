use std::backtrace::Backtrace;

use uwu::utils::single_instance;

use winit::event_loop::{ControlFlow, EventLoop};

#[cfg(feature = "mimalloc")]
use mimalloc::MiMalloc;

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[cfg(not(target_os = "android"))]
fn main() {
    #[cfg(target_os = "linux")]
    uwu::utils::linux::silence_glib_logs();

    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::System::Console::FreeConsole();
    }

    #[cfg(feature = "profiling")]
    puffin::set_scopes_on(true);

    std::panic::set_hook(Box::new(|info| {
        eprintln!("panic: {info}");
        eprintln!("backtrace:\n{}", Backtrace::force_capture());

        rfd::MessageDialog::new()
            .set_title("应用崩溃")
            .set_level(rfd::MessageLevel::Error)
            .set_description(info.to_string())
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
    }));

    println!(
        r"
          __  ___      ____  __
         / / / / | /| / / / / /
        / /_/ /| |/ |/ / /_/ / 
        \__,_/ |__/|__/\__,_/  
    "
    );
    println!(
        "
   \x1b[3mujhhgtg's whiteboard, unleashed\x1b[0m
    "
    );

    pollster::block_on(run_desktop());
}

#[cfg(not(target_os = "android"))]
async fn run_desktop() {
    let arg_path = std::env::args().nth(1).map(|p| {
        std::path::Path::new(&p)
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from(p))
    });

    match single_instance::check(arg_path.clone()) {
        Ok(single_instance::SingleInstance::NotFirst) => {
            println!("another instance is already running");
            return;
        }
        Ok(single_instance::SingleInstance::First) => {}
        Err(e) => {
            eprintln!("single-instance check failed: {e}");
        }
    }

    #[cfg(target_os = "linux")]
    uwu::utils::associations::check_and_update_linux_desktop_file();

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    if let Ok(mut lock) = single_instance::EVENT_LOOP_PROXY.lock() {
        *lock = Some(event_loop.create_proxy());
    }

    let mut app = uwu::app::App::new();
    app.state.initial_file = arg_path;
    event_loop.run_app(&mut app).expect("failed to run app");
}
