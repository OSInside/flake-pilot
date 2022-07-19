use std::process::Command;

pub fn load(oci: &String) -> i32 {
    /*!
    Call podman load with the provided oci tar file
    !*/
    let mut status_code = 255;

    info!("Loading OCI container...");
    info!("podman load -i {}", oci);
    let status = Command::new("podman")
        .arg("load")
        .arg("-i")
        .arg(oci)
        .status();

    match status {
        Ok(status) => {
            status_code = status.code().unwrap();
            if ! status.success() {
                error!("Failed, error message(s) reported");
            }
        }
        Err(status) => { error!("Process terminated by signal: {}", status) }
    }

    status_code
}