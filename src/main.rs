use axum::{
	routing::post,
	http::StatusCode,
	Router,
	extract::Multipart,
};

use std::{
	net::SocketAddr,
	process::Command,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
	let app = Router::new()
		.route("/upload", post(upload_file)); //register the /upload route to the proper handler function

	let addr = SocketAddr::from(([0,0,0,0],8001));
	axum::Server::bind(&addr)
	.serve(app.into_make_service())
	.await
	.unwrap();
}


async fn upload_file(mut multipart: Multipart) -> (StatusCode, String) {
	while let Some(field) = multipart.next_field().await.unwrap() {
		match field.name().unwrap().to_string().as_str() {
			"elfFile"=> {
				let file_name = field.file_name().unwrap();
				let file_extension = file_name.split(".").last().unwrap();
				if file_extension != "elf" {
					return (StatusCode::NOT_ACCEPTABLE, "Wrong file extension detected! Check your file!\n".to_string());
				}

				let restart = if Command::new("systemctl").arg("is-active").arg("--quiet").arg("go-simulink").status().unwrap().success() {
					_ = Command::new("systemctl").arg("stop").arg("go-simulink").status();
					true
				} else {
					false
				};
				_ = Command::new("rm").arg("/usr/simulink/*").status();

				if !std::path::Path::new("/usr/simulink").try_exists().unwrap() {
					std::fs::create_dir("/usr/simulink").unwrap();
				}

				match std::fs::write("/usr/simulink/GOcontroll_Linux.elf", field.bytes().await.unwrap()) {
					Ok(_) => {
						if restart {
							_ = Command::new("systemctl").arg("start").arg("go-simulink").spawn();
						}
						_ = Command::new("sync").spawn();
						return (StatusCode::OK, "File uploaded! You can now close this tab/window.\n".to_string());
					},
					Err(err) => {
						return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write file to server.\n{}\n",err));
					}
				}
			},
			"a2lFile"=> {
				let file_name = field.file_name().unwrap();
				let file_extension = file_name.split(".").last().unwrap();
				if file_extension != "a2l" {
					return (StatusCode::NOT_ACCEPTABLE, "Wrong file extension detected! Check your file!\n".to_string());
				}
				match std::fs::write("/usr/simulink/GOcontroll_Linux.a2l", field.bytes().await.unwrap()) {
					Ok(_) => {
						_ = Command::new("systemctl").arg("restart").arg("nodered").spawn();
						_ = Command::new("sync").spawn();
						return (StatusCode::OK, "File uploaded! You can now close this tab/window.\n".to_string());
					},
					Err(err) => {
						return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write file to server.\n{}\n",err));
					},
				}
			},
			"ovpnFile"=> {
				let file_name = field.file_name().unwrap();
				let file_extension = file_name.split(".").last().unwrap();
				if file_extension != "ovpn" {
					return (StatusCode::NOT_ACCEPTABLE, "Wrong file extension detected! Check your file!\n".to_string());
				}

				if !std::path::Path::new("/etc/openvpn").try_exists().unwrap() {
					std::fs::create_dir("/etc/openvpn").unwrap();
				}

				match std::fs::write("/etc/openvpn/moduline.conf", field.bytes().await.unwrap()) {
					Ok(_) => if Command::new("systemctl").arg("is-active").arg("--quiet").arg("openvpn").status().unwrap().success() {
						_ = Command::new("systemctl").arg("restart").arg("openvpn").spawn();
						_ = Command::new("sync").spawn();
						return (StatusCode::OK, "File uploaded! You can now close this tab/window.\n".to_string());
					},
					Err(err) => {
						return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to write file to server.\n{}\n",err));
					}
				}
			}
			_=> {
				return (StatusCode::IM_A_TEAPOT, "Unknown file type received\n".to_string());
			}
		}
	}
	(StatusCode::OK, "file received\n".to_string())
}