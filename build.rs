// Embed a losslessly compressed native icon; PNG encoding runs only at build time.
use std::{env, fs, path::PathBuf};
fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}
fn resource(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    u32le(out, data.len() as u32);
    u32le(out, 32);
    for value in [0xffff, kind, 0xffff, id] {
        u16le(out, value);
    }
    u32le(out, 0);
    u16le(out, 0x1030);
    u16le(out, 0);
    u32le(out, 0);
    u32le(out, 0);
    out.extend(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut rgba = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32i32 {
        for x in 0..32i32 {
            let dx = if x < 7 { 7 - x } else { (x - 24).max(0) };
            let dy = if y < 7 { 7 - y } else { (y - 24).max(0) };
            if dx * dx + dy * dy <= 49 {
                let white = ((x == 9 || x == 10) && (6..=22).contains(&y))
                    || ((y == 21 || y == 22) && (9..=26).contains(&x))
                    || ((y == 9 || y == 10) && (6..=22).contains(&x))
                    || ((x == 21 || x == 22) && (9..=26).contains(&y));
                rgba.extend(if white {
                    [255, 255, 255, 255]
                } else {
                    [51, 112, 255, 255]
                });
            } else {
                rgba.extend([0, 0, 0, 0]);
            }
        }
    }
    let mut icon = Vec::new();
    let mut encoder = png::Encoder::new(&mut icon, 32, 32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&rgba).unwrap();
    writer.finish().unwrap();
    let mut group = Vec::new();
    for value in [0, 1, 1] {
        u16le(&mut group, value);
    }
    group.extend([32, 32, 0, 0]);
    u16le(&mut group, 1);
    u16le(&mut group, 32);
    u32le(&mut group, icon.len() as u32);
    u16le(&mut group, 1);
    let mut res = Vec::new();
    resource(&mut res, 0, 0, &[]);
    resource(&mut res, 3, 1, &icon);
    resource(&mut res, 14, 1, &group);
    resource(&mut res, 24, 1, br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="0.1.0.0" processorArchitecture="*" name="Qingjie.Desktop" type="win32"/>
  <dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3"><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security></trustInfo>
</assembly>"#);
    let path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("qingjie.res");
    fs::write(&path, res).unwrap();
    println!("cargo:rustc-link-arg={}", path.display());
}
