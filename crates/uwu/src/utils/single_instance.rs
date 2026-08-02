use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};

pub static FOCUS_REQUESTED: AtomicBool = AtomicBool::new(false);
pub static FILES_TO_OPEN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
pub static EVENT_LOOP_PROXY: Mutex<Option<winit::event_loop::EventLoopProxy<()>>> =
    Mutex::new(None);

const APP_ID: &str = "uwu-ujhhgtg-whiteboard";

pub enum SingleInstance {
    First,
    NotFirst,
}

pub fn wakeup_event_loop() {
    if let Ok(lock) = EVENT_LOOP_PROXY.lock()
        && let Some(proxy) = &*lock
    {
        let _ = proxy.send_event(());
    }
}

pub fn check(arg_path: Option<PathBuf>) -> std::io::Result<SingleInstance> {
    let make_name = || APP_ID.to_ns_name::<GenericNamespaced>();

    fn spawn_listener(listener: interprocess::local_socket::Listener) -> SingleInstance {
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                match conn {
                    Ok(stream) => {
                        let mut reader = BufReader::new(stream);
                        let mut cmd = String::new();
                        if reader.read_line(&mut cmd).is_ok() {
                            let cmd = cmd.trim();
                            if cmd == "focus" {
                                FOCUS_REQUESTED.store(true, Ordering::Relaxed);
                                wakeup_event_loop();
                            } else if cmd == "open" {
                                let mut path_str = String::new();
                                if reader.read_line(&mut path_str).is_ok() {
                                    let path = PathBuf::from(path_str.trim());
                                    if let Ok(mut lock) = FILES_TO_OPEN.lock() {
                                        lock.push(path);
                                    }
                                    FOCUS_REQUESTED.store(true, Ordering::Relaxed);
                                    wakeup_event_loop();
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("single-instance listener error: {e}");
                    }
                }
            }
        });
        SingleInstance::First
    }

    let try_listener = || {
        ListenerOptions::new()
            .name(make_name()?)
            .create_sync()
            .map(spawn_listener)
    };

    match try_listener() {
        Ok(instance) => Ok(instance),
        Err(_) => match Stream::connect(make_name()?) {
            Ok(mut stream) => {
                if let Some(path) = arg_path {
                    stream.write_all(format!("open\n{}\n", path.display()).as_bytes())?;
                } else {
                    stream.write_all(b"focus\n\n")?;
                }
                stream.flush()?;
                Ok(SingleInstance::NotFirst)
            }
            // Connecting failed too: either a stale socket is still around or
            // the first instance is mid-initialization. Retry becoming the
            // listener once before giving up.
            Err(_) => try_listener(),
        },
    }
}
