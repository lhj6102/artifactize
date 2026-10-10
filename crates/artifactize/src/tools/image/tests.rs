use std::fs;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

use super::*;
use crate::{
    test_os::{symlink_dir, symlink_file},
    tools::result,
};

pub(in crate::tools) fn fixtures() -> [(&'static str, Vec<u8>); 3] {
    let mut png = PNG_SIGNATURE.to_vec();
    chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
    chunk(&mut png, b"tEXt", b"note\0acTL");
    chunk(
        &mut png,
        b"IDAT",
        &[0x78, 1, 1, 2, 0, 0xfd, 0xff, 0, 0xff, 1, 1, 1, 0],
    );
    chunk(&mut png, b"IEND", &[]);
    // One-pixel white JPEG and WebP, materialized in memory without decoder dependencies.
    let jpeg = STANDARD
        .decode(
            "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAMCAgICAgMCAgIDAwMDBAYEBAQEBAgGBgUGCQgKCgkICQkKDA8MCgsOCwkJDRENDg8QEBEQCgwSExIQEw8QEBD/wAALCAABAAEBAREA/8QAFAABAAAAAAAAAAAAAAAAAAAACf/EABQQAQAAAAAAAAAAAAAAAAAAAAD/2gAIAQEAAD8AVN//2Q==",
        )
        .unwrap();
    let webp = STANDARD
        .decode("UklGRiQAAABXRUJQVlA4IBgAAAAwAQCdASoBAAEAAgA0JaQAA3AA/vuUAAA=")
        .unwrap();
    [
        ("image/png", png),
        ("image/jpeg", jpeg),
        ("image/webp", webp),
    ]
}

fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = u32::MAX;
    for byte in kind.iter().chain(data) {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 * (crc & 1));
        }
    }
    png.extend_from_slice(&(!crc).to_be_bytes());
}

pub(in crate::tools) fn unsupported() -> Vec<Vec<u8>> {
    let png = &fixtures()[0].1;
    let mut animated = png[..33].to_vec();
    chunk(&mut animated, b"acTL", &[0, 0, 0, 1, 0, 0, 0, 0]);
    animated.extend_from_slice(&png[33..]);
    let mut bad_png = png.clone();
    bad_png[12] |= 0x80;
    let mut bad_webp = fixtures()[2].1.clone();
    bad_webp[8] |= 0x80;
    let mut bad_length = png.clone();
    bad_length[33..37].copy_from_slice(&u32::MAX.to_be_bytes());
    vec![
        vec![],
        b"not an image".to_vec(),
        b"GIF89a\x01\0\x01\0".to_vec(),
        b"BM\0\0\0\0".to_vec(),
        b"\xff\xd8\xff\xf7".to_vec(),
        PNG_SIGNATURE.to_vec(),
        png[..20].to_vec(),
        animated,
        bad_png,
        bad_webp,
        bad_length,
    ]
}

#[test]
fn image_blocks_normalize_all_formats_and_serialize_without_paths() {
    let directory = tempfile::tempdir().unwrap();
    for (mime, bytes) in fixtures() {
        let encoded = STANDARD.encode(&bytes);
        let block = json!({"type":"image","data":encoded,"mimeType":mime});
        let file = directory.path().join("nested/image.wrong-extension");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, &bytes).unwrap();
        for source in [
            block.clone(),
            json!({"type":"image","data":encoded.trim_end_matches('='),"mimeType":mime}),
            json!({"type":"image","path":"nested/image.wrong-extension","mimeType":mime}),
            json!({"type":"image","path":file,"mimeType":mime}),
        ] {
            let result = result::parse(
                &serde_json::to_vec(&json!({"content":[source]})).unwrap(),
                directory.path(),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(&result).unwrap(),
                json!({"content":[block]})
            );
            let roundtrip =
                serde_json::from_value::<crate::tools::ToolResult>(json!({"content":[block]}))
                    .unwrap();
            assert_eq!(result, roundtrip);
        }
    }
}

#[test]
fn image_validation_rejects_unsupported_signatures_mime_and_base64() {
    for bytes in unsupported() {
        assert!(normalize(&bytes, None).is_err());
        for mime in ["image/png", "image/jpeg", "image/webp"] {
            assert!(from_base64(&STANDARD.encode(&bytes), mime).is_err());
        }
    }
    for (mime, bytes) in fixtures() {
        for other in [
            "image/png",
            "image/jpeg",
            "image/webp",
            "image/gif",
            "IMAGE/PNG",
            "",
        ] {
            if other != mime {
                assert!(from_base64(&STANDARD.encode(&bytes), other).is_err());
            }
        }
    }
    for invalid in [
        "",
        "=",
        "%%%",
        "abcd===",
        "data:image/png;base64,aGVsbG8=",
        " aGVsbG8=",
    ] {
        assert!(from_base64(invalid, "image/png").is_err());
    }
    let output = tempfile::tempdir().unwrap();
    let encoded = STANDARD.encode(&fixtures()[0].1);
    for block in [
        json!({"type":"image","mimeType":"image/png"}),
        json!({"type":"image","mimeType":"image/png","data":encoded,"path":"file"}),
        json!({"type":"image","mimeType":"image/png","data":encoded,"extra":1}),
        json!({"type":"image","mimeType":"image/png","data":42}),
    ] {
        assert!(
            result::parse(
                &serde_json::to_vec(&json!({"content":[block]})).unwrap(),
                output.path()
            )
            .is_err()
        );
    }
    assert!(
        result::parse(
            &serde_json::to_vec(&json!({
                "isError":true,
                "content":[{"type":"image","mimeType":"image/png","data":encoded}],
            }))
            .unwrap(),
            output.path()
        )
        .is_err()
    );
}

#[test]
fn decoded_limit_is_inclusive_and_normalized_result_size_is_bounded() {
    let output = tempfile::tempdir().unwrap();
    let mut bytes = fixtures()[1].1.clone();
    bytes.resize(IMAGE_LIMIT, 0);
    let file = output.path().join("large");
    fs::write(&file, &bytes).unwrap();
    assert!(from_base64(&STANDARD.encode(&bytes), "image/jpeg").is_ok());
    assert!(from_output(output.path(), "large", "image/jpeg").is_ok());
    let block = json!({"type":"image","mimeType":"image/jpeg","path":"large"});
    assert!(
        result::parse(
            &serde_json::to_vec(&json!({"content":[block.clone(),block]})).unwrap(),
            output.path()
        )
        .is_err()
    );
    bytes.push(0);
    assert!(from_base64(&STANDARD.encode(&bytes), "image/jpeg").is_err());
    fs::write(&file, &bytes).unwrap();
    assert!(from_output(output.path(), "large", "image/jpeg").is_err());
    bytes.resize(IMAGE_LIMIT + 3, 0);
    assert!(from_base64(&STANDARD.encode(&bytes), "image/jpeg").is_err());
}

#[test]
fn output_paths_reject_traversal_symlinks_and_nonregular_files() {
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("output");
    fs::create_dir_all(output.join("nested")).unwrap();
    let bytes = &fixtures()[0].1;
    fs::write(output.join("image"), bytes).unwrap();
    fs::write(directory.path().join("outside"), bytes).unwrap();
    fs::write(output.join("nested/image"), bytes).unwrap();
    let outside = directory.path().join("outside");
    let other = directory.path().join("output-other/image");
    let mut paths = vec![
        "../outside",
        "nested/../../outside",
        "nested/../image",
        "nested",
        "",
        "./image",
        "nested\\image",
        "missing",
        outside.to_str().unwrap(),
        other.to_str().unwrap(),
    ];
    if symlink_file("image", output.join("link")).is_some() {
        paths.push("link");
    }
    if symlink_dir("nested", output.join("linkdir")).is_some() {
        paths.push("linkdir/image");
    }
    if symlink_file(
        std::path::Path::new("..").join("outside"),
        output.join("escape"),
    )
    .is_some()
    {
        paths.push("escape");
    }
    #[cfg(unix)]
    {
        let fifo = std::ffi::CString::new(output.join("fifo").to_str().unwrap()).unwrap();
        // SAFETY: CString supplies a live NUL-terminated path in this test's private
        // temporary directory; mkfifo retains no pointer and 0600 is a valid mode.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        paths.push("fifo");
    }
    #[cfg(windows)]
    {
        crate::test_os::junction(&output.join("nested"), &output.join("joined"));
        paths.push("joined/image");
    }
    for path in paths {
        assert!(from_output(&output, path, "image/png").is_err(), "{path}");
    }
    fs::rename(&output, directory.path().join("old-output")).unwrap();
    if symlink_dir("old-output", &output).is_some() {
        assert!(from_output(&output, "image", "image/png").is_err());
    }
    #[cfg(windows)]
    {
        let _ = fs::remove_dir(&output);
        crate::test_os::junction(&directory.path().join("old-output"), &output);
        assert!(from_output(&output, "image", "image/png").is_err());
    }
}
