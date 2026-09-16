//! Build-time Tauri configuration for the Mesh desktop shell.

const DEVELOPMENT_ICON_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAIAAAACACAYAAADDPmHLAAAA8klEQVR42u3SMQ0AAAjAMPTgA1OYBxsk9JiBpVHZo7+FCQAYAYAAEAACQAAIAAEgAASAABAAAkAACAABIAAEgAAQAAJAAAgAASAABIAAEAACQAAIAAEgAASAABAAAkAACAABIAAEgAAQAAJAAAgAASAABIAAEAACQAAIAAEgAASAABAAAkAACAABIAAEgAAQAAJAAAgAASAABIAAEAACQAAIAAEgAASAABAAAgAAEwAwAgABIAAEgAAQAAJAAAgAASAABIAAEAACQAAIAAEgAASAABAAAkAACAABIAAEgAAQAAJAAAgAASAABIAAEAACQDdaOxoHJw4nDRIAAAAASUVORK5CYII=";

fn decode_base64(input: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => panic!("invalid generated icon base64"),
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((accumulator >> bits) as u8);
            accumulator &= (1_u32 << bits) - 1;
        }
    }
    output
}

fn main() {
    println!("cargo:rerun-if-env-changed=MESH_BUILD_REVISION");
    let revision = std::env::var("MESH_BUILD_REVISION").unwrap_or_else(|_| "development".into());
    assert!(
        revision == "development"
            || (revision.len() == 40
                && revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))),
        "MESH_BUILD_REVISION must be 'development' or one canonical lowercase Git commit"
    );
    println!("cargo:rustc-env=MESH_BUILD_REVISION={revision}");

    // `generate_context!` and the local `.app` bundle require one PNG. Keep this generated,
    // intentionally unbranded development icon out of source control; release branding, signing,
    // notarization and distribution remain separate work.
    let icon = std::path::Path::new("icons/icon.png");
    std::fs::create_dir_all("icons").expect("create generated icon directory");
    std::fs::write(icon, decode_base64(DEVELOPMENT_ICON_BASE64))
        .expect("write generated development icon");
    tauri_build::build();
}
