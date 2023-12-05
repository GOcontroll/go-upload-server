use std::process::{Command, exit};

extern crate tiny_http;
extern crate multipart;

use std::io::{self, Cursor};
use multipart::server::{Multipart, SaveResult};
use tiny_http::{Response, StatusCode, Request};
fn main() {
    // Starting a server on `localhost:80`
    let server = tiny_http::Server::http("0.0.0.0:8001").unwrap_or_else(|err| {
		eprintln!("Could not bind 0.0.0.0:8001\n{}", err);
		exit(-1);
	});
    loop {
        // This blocks until the next request is received
        let mut request = server.recv().unwrap();

        // Processes a request and returns response or an occured error
        let result = process_request(&mut request);
        let resp = match result {
            Ok(resp) => resp,
            Err(e) => {
                println!("An error has occured during request proccessing: {:?}", e);
                build_response(500, "The received data was not correctly proccessed on the server")
            }
        };

        // Answers with a response to a client
        _ = request.respond(resp);
    }
}

type RespBody = Cursor<Vec<u8>>;

/// Processes a request and returns response or an occured error.
fn process_request(request: &mut Request) -> io::Result<Response<RespBody>> {
    // Getting a multipart reader wrapper
    match Multipart::from_request(request) {
        Ok(mut multipart) => {
			while let Ok(Some(mut field)) = multipart.read_entry() {
				match field.headers.name.as_ref(){
					"elfFile"=> {
						let file_name = field.headers.filename.unwrap();
						let file_extension = file_name.split(".").last().unwrap();
						if file_extension != "elf" {
							return Ok(build_response(400, "Wrong file extension detected! Check your file!"));
						}

						let restart = if Command::new("systemctl").arg("is-active").arg("--quiet").arg("go-simulink").status().unwrap().success() {
							_ = Command::new("systemctl").arg("stop").arg("go-simulink").status();
							true
						} else {
							false
						};

						_ = std::fs::remove_file("/usr/simulink/GOcontroll_Linux.elf");

						if !std::path::Path::new("/usr/simulink").try_exists().unwrap() {
							std::fs::create_dir("/usr/simulink").unwrap();
						}

						match field.data.save().ignore_text().with_path("/usr/simulink/GOcontroll_Linux.elf") {
							SaveResult::Full(_) => {
								if restart {
									_ = Command::new("systemctl").arg("start").arg("go-simulink").spawn();
								}
								_ = Command::new("sync").spawn();
								return Ok(build_response(200, "File uploaded! You can now close this tab/window."));
							},
							SaveResult::Error(err) => {
								return Ok(build_response(400, format!("Failed to write file to server.\n{}",err)));
							},
							SaveResult::Partial(_res, err) => {
								return Ok(build_response(400, format!("Failed to write file to server.\n{:?}",err)));
							}
						}
					},
					"a2lFile"=> {
						let file_name = field.headers.filename.unwrap();
						let file_extension = file_name.split(".").last().unwrap();
						if file_extension != "a2l" {
							return Ok(build_response(400, "Wrong file extension detected! Check your file!"));
						}

						_ = std::fs::remove_file("/usr/simulink/GOcontroll_Linux.a2l");

						match field.data.save().with_path("/usr/simulink/GOcontroll_Linux.a2l") {
							SaveResult::Full(_) => {
								_ = Command::new("systemctl").arg("restart").arg("nodered").spawn();
								_ = Command::new("sync").spawn();
								return Ok(build_response(200, "File uploaded! You can now close this tab/window."));
							},
							SaveResult::Error(err) => {
								return Ok(build_response(400, format!("Failed to write file to server.\n{}",err)));
							},
							SaveResult::Partial(_res, err) => {
								return Ok(build_response(400, format!("Failed to write file to server.\n{:?}",err)));
							}
						}
					},
					"ovpnFile"=> {
						let file_name = field.headers.filename.unwrap();
						let file_extension = file_name.split(".").last().unwrap();
						if file_extension != "ovpn" {
							return Ok(build_response(400, "Wrong file extension detected! Check your file!"));
						}

						if !std::path::Path::new("/etc/openvpn").try_exists().unwrap() {
							std::fs::create_dir("/etc/openvpn").unwrap();
						}

						_ = std::fs::remove_file("/etc/openvpn/moduline.conf");

						match field.data.save().with_path("/etc/openvpn/moduline.conf") {
							SaveResult::Full(_) => if Command::new("systemctl").arg("is-active").arg("--quiet").arg("openvpn").status().unwrap().success() {
								_ = Command::new("systemctl").arg("restart").arg("openvpn").spawn();
								_ = Command::new("sync").spawn();
								return Ok(build_response(200, "File uploaded! You can now close this tab/window."));
							},
							SaveResult::Error(err) => {
								return Ok(build_response(400, format!("Failed to write file to server.\n{}",err)));
							},
							SaveResult::Partial(_res, err) => {
								return Ok(build_response(400, format!("Failed to write file to server.\n{:?}",err)));
							}
						}
					}
					_=> {
						return Ok(build_response(400, "Unknown file type received\n"));
					}
				}
			}
			return Ok(build_response(200, "File uploaded! You can now close this tab/window."));
        }
        Err(_) => Ok(build_response(400, "The request is not multipart")),
    }
}

fn build_response<D: Into<Vec<u8>>>(status_code: u16, data: D) -> Response<RespBody> {
    let data = data.into();
    let data_len = data.len();
    Response::new(StatusCode(status_code),
                  vec![],
                  Cursor::new(data),
                  Some(data_len),
                  None)
}