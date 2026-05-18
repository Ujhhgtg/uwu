use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName};

pub static FOCUS_REQUESTED: AtomicBool = AtomicBool::new(false);

const APP_ID: &str = "uwu-ujhhgtg-whiteboard";

pub enum SingleInstance {
    First,
    NotFirst,
}

pub fn check() -> std::io::Result<SingleInstance> {
    let make_name = || APP_ID.to_ns_name::<GenericNamespaced>();

    match ListenerOptions::new().name(make_name()?).create_sync() {
        Ok(listener) => {
            std::thread::spawn(move || {
                for conn in listener.incoming() {
                    match conn {
                        Ok(stream) => {
                            let mut reader = BufReader::new(stream);
                            let mut line = String::new();
                            if reader.read_line(&mut line).is_ok() && line.trim() == "focus" {
                                FOCUS_REQUESTED.store(true, Ordering::Relaxed);
                            }
                        }
                        Err(e) => {
                            eprintln!("single-instance listener error: {e}");
                        }
                    }
                }
            });
            Ok(SingleInstance::First)
        }
        Err(_) => {
            let mut stream = Stream::connect(make_name()?)?;
            stream.write_all(b"focus\n")?;
            stream.flush()?;
            Ok(SingleInstance::NotFirst)
        }
    }
}
