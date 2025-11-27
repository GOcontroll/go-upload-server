use std::process::Command;

use axum::{extract::Multipart, http::StatusCode, response::Response, routing::post, Router};

use tokio::net::TcpListener;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("GOcontroll upload server V{}", VERSION);
    let app = Router::new().route("/upload", post(handle_upload));
    let listener = TcpListener::bind("0.0.0.0:8001").await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}

async fn handle_upload(mut multipart: Multipart) -> Response<String> {
    while let Some(field) = multipart.next_field().await.unwrap() {
        match field.name().unwrap() {
            "elfFile" => {
                let Some(file_name) = field.file_name() else {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "No filename given, can't process\n".to_owned(),
                    );
                };
                let Some(file_extension) = file_name.split(".").last() else {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "Could not get file extension, can't process\n".to_owned(),
                    );
                };
                if file_extension != "elf" {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "Wrong file extension detected! Check your file!\n".to_owned(),
                    );
                }

                let restart = if Command::new("systemctl")
                    .arg("is-active")
                    .arg("--quiet")
                    .arg("go-simulink")
                    .status()
                    .unwrap()
                    .success()
                {
                    _ = Command::new("systemctl")
                        .arg("stop")
                        .arg("go-simulink")
                        .status();
                    true
                } else {
                    false
                };

                _ = std::fs::remove_file("/usr/simulink/GOcontroll_Linux.elf");

                if !std::path::Path::new("/usr/simulink")
                    .try_exists()
                    .expect("can't check the existence of /usr/simulink")
                {
                    std::fs::create_dir("/usr/simulink").expect("could not create /usr/simulink");
                }

                let bytes = match field.bytes().await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return build_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    }
                };

                match std::fs::write("/usr/simulink/GOcontroll_Linux.elf", bytes) {
                    Ok(_) => {
                        _ = Command::new("chmod")
                            .args(["555", "/usr/simulink/GOcontroll_Linux.elf"])
                            .status();
                        if restart {
                            _ = Command::new("systemctl")
                                .arg("start")
                                .arg("go-simulink")
                                .spawn();
                        }
                        _ = Command::new("sync").spawn();
                        return build_response(StatusCode::OK, "elfFile uploaded!\n".to_owned());
                    }
                    Err(err) => {
                        return build_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to write file to server.\n{}", err.to_string()),
                        );
                    }
                }
            }
            "a2lFile" => {
                let Some(file_name) = field.file_name() else {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "No filename given, can't process\n".to_owned(),
                    );
                };
                let Some(file_extension) = file_name.split(".").last() else {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "Could not get file extension, can't process\n".to_owned(),
                    );
                };
                if file_extension != "a2l" {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "Wrong file extension detected! Check your file!\n".to_owned(),
                    );
                }

                _ = std::fs::remove_file("/usr/simulink/GOcontroll_Linux.a2l");

                let bytes = match field.bytes().await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return build_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    }
                };

                match std::fs::write("/usr/simulink/GOcontroll_Linux.a2l", bytes) {
                    Ok(_) => {
                        _ = Command::new("go-parse-a2l")
                            .spawn();
                        _ = Command::new("systemctl")
                            .arg("restart")
                            .arg("nodered")
                            .spawn();
                        _ = Command::new("sync").spawn();
                        return build_response(StatusCode::OK, "a2lFile uploaded!\nParsed a2l\n".to_owned());
                    }
                    Err(err) => {
                        return build_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to write file to server.\n{}", err.to_string()),
                        );
                    }
                }
            }
            "ovpnFile" => {
                let Some(file_name) = field.file_name() else {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "No filename given, can't process\n".to_owned(),
                    );
                };
                let Some(file_extension) = file_name.split(".").last() else {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "Could not get file extension, can't process\n".to_owned(),
                    );
                };
                if file_extension != "ovpn" {
                    return build_response(
                        StatusCode::BAD_REQUEST,
                        "Wrong file extension detected! Check your file!\n".to_owned(),
                    );
                }

                if !std::path::Path::new("/etc/openvpn")
                    .try_exists()
                    .expect("can't check the existence of /etc/openvpn")
                {
                    std::fs::create_dir("/etc/openvpn").expect("can't create /etc/openvpn");
                }

                _ = std::fs::remove_file("/etc/openvpn/moduline.conf");

                let bytes = match field.bytes().await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        return build_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
                    }
                };

                match std::fs::write("/etc/openvpn/moduline.conf", bytes) {
                    Ok(_) => {
                        if Command::new("systemctl")
                            .arg("is-active")
                            .arg("--quiet")
                            .arg("openvpn")
                            .status()
                            .unwrap()
                            .success()
                        {
                            _ = Command::new("systemctl")
                                .arg("restart")
                                .arg("openvpn")
                                .spawn();
                            _ = Command::new("sync").spawn();
                            return build_response(StatusCode::OK, "ovpnFile uploaded!\n".to_owned());
                        }
                    }
                    Err(err) => {
                        return build_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to write file to server.\n{}", err.to_string()),
                        );
                    }
                }
            }
            _ => {
                return build_response(
                    StatusCode::BAD_REQUEST,
                    "Could not process upload\n".to_owned(),
                )
            }
        }
    }
    build_response(
        StatusCode::BAD_REQUEST,
        "Could not process upload\n".to_owned(),
    )
}

fn build_response(status_code: StatusCode, message: String) -> Response<String> {
    Response::builder()
        .status(status_code)
        .body(message)
        .unwrap()
}
