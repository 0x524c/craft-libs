use std::path::Path;

use craft_docpath::{PathKind, Refused, classify, is_local, resolve, resolve_absolute};

fn check(kind: PathKind, cases: &[&str]) {
    for raw in cases {
        assert_eq!(classify(raw), kind, "{raw:?}");
        assert_eq!(is_local(raw), kind == PathKind::Local, "{raw:?}");
    }
}

#[test]
fn local_paths() {
    check(
        PathKind::Local,
        &[
            r"C:\x",
            "C:/x",
            r"c:\Users\me\Pictures\a.png",
            r"\\?\C:\x",
            r"\\?\C:\",
            r"\\?\D:\long\folder\file.txt.",
            "/home/u/x",
            "/",
            "img/a.png",
            r"img\a.png",
            "./a.png",
            "../shared/a.png",
            "C:x",
            "C:",
            r"\x",
            "console.txt",
            "nul_report.txt",
            "com10.txt",
            "lpt.txt",
            "auxiliary/a.png",
            "file:///C:/x/a.png",
            "file:///home/u/a.png",
            "file://localhost/C:/x",
            "FILE://LOCALHOST/home/u/x",
            "file:/C|/x",
            "file:///C:/My%20Pictures/a.png",
            "file:img/a.png",
            "file://C:/x",
            "file://c|/x",
            r"file:\\C:\x",
            "file:///C:/caf%E9.png",
            "file:///C:/%FF",
            r"C:\\host\share",
            r"./\\host\share\x",
            "D:/x",
            "z:",
            "file:///D:",
            r"\\?\Z:\x",
        ],
    );
}

#[test]
fn network_paths() {
    check(
        PathKind::Network,
        &[
            r"\\h\s",
            r"\\h\s\x.png",
            "//h/s/x.png",
            r"\/h\s",
            r"/\h\s",
            r"\\h",
            r"\\",
            r"\\\h\s",
            r"\\?\UNC\h\s\x",
            r"\\?\unc\h\s",
            "//?/UNC/h/s/x",
            r"\\.\UNC\h\s\x",
            r"\\h@SSL\s\x",
            r"\\h@80\s\x",
            r"\\h@SSL@443\DavWWWRoot\x",
            "file://h/s/x.png",
            "file:////h/s/x.png",
            r"file:\\h\s\x.png",
            "file:%5C%5Ch%5Cs%5Cx.png",
            "file:///%5C%5Ch/s/x.png",
            "file://%5C%5Ch/s",
            "file:%2F%2Fh/s/x",
        ],
    );
}

#[test]
fn device_paths() {
    check(
        PathKind::Device,
        &[
            r"\\.\C:",
            r"\\.\PhysicalDrive0",
            r"\\.\pipe\x",
            "//./pipe/x",
            r"\\.",
            r"\??\C:\x",
            "/??/C:/x",
            r"\??",
            r"\\?",
            r"\\?\",
            r"\\?\C:",
            r"\\?\C:x",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\x",
            r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\x",
            "//?/C:/x",
            r"\/?\C:\x",
            r"/\?\C:\x",
            r"\\?/C:\x",
            "CON",
            "con.txt",
            "Nul",
            "nul.tar.gz",
            "PRN...",
            "aux .png",
            "COM1",
            "com9.log",
            "COM¹",
            "LPT1",
            "lpt9.txt",
            "CONIN$",
            "conout$",
            "NUL:",
            r"C:\docs\nul",
            "img/CON/a.png",
            r"\\?\C:\docs\con.txt",
            "file:///C:/nul",
        ],
    );
}

#[test]
fn url_links() {
    check(
        PathKind::Url,
        &[
            "http://h/x.png",
            "https://h/x.png",
            "HTTPS://h/x.png",
            "ms-word:ofe|u|https://h/x.docx",
            "smb://h/s/x",
            "webdav://h/x",
            "mailto:a@b",
            "svn+ssh://h/x",
            "file:http://h/x",
            r"http:\\h\x",
            "data:image/png;base64,AAAA",
            "javascript:alert(1)",
            "VBScript:x",
            "ms-excel:ofe|u|https://h/x.xlsx",
            "search-ms:query=x&crumb=location:\\\\h\\s",
            "shell:startup",
            "tel:123",
            "news:comp.lang.rust",
            "jar:file:///C:/x.jar!/a.png",
            "view-source:http://h/x",
            "name:stream",
            "chapter1:intro.png",
            "x.png:stream",
            "ab:c",
            "file:jar:x",
            "a1+b-c.d:x",
        ],
    );
}

#[test]
fn invalid_paths() {
    check(
        PathKind::Invalid,
        &[
            "",
            "a\0b",
            "a\nb",
            "a\tb",
            "a\u{7f}",
            "a\u{85}b",
            "file:",
            "file:///C:/a%00b",
            "file:///C:/a%0Ab",
            r"C:\..\x",
            r"C:\a\..\..\x",
            "/..",
            r"\\?\C:\a\..\b",
            r"\\?\C:\a\.\b",
            r"\\?\C:\a/b",
            r"\\?\C:\a\\b",
            r"C:\a\.. \b",
            r"C:\a\...\b",
            "a/. /b",
            "file://",
            "file://localhost",
            "file://?x",
            "file:#x",
            "file:///C:/%81",
            "a\u{FF3C}b",
            "\\\u{FF3C}h\\s",
            "a\u{FF0F}b",
            "a\u{2215}b",
            "a\u{29F5}b",
            "file:///C:/a%EF%BC%BCb",
            "..:",
            ".:x",
            r"C:\a\..:x",
            r"\\?\C:\a\..:x",
            "a/b:c.png",
            "img/fig:1.png",
            r"C:\x:stream",
            r"C:\a\.b:c",
            "C:x:y",
            "C::x",
            "/home/u/a:b",
            "file:///C:/a:b",
            "file:///C:/a%3Ab",
            r"\\?\C:\a:b",
            ":x",
            "1a:b",
            "./ab:c",
        ],
    );
}

fn slashed(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[test]
fn resolve_joins_relative_paths_to_the_document_folder() {
    let doc = Path::new(r"C:\docs\report\a.doc");
    let ok = |raw: &str| slashed(&resolve(raw, Some(doc)).unwrap());
    assert_eq!(resolve(r"img\x.png", Some(doc)).unwrap(), Path::new(r"C:\docs\report\img\x.png"));
    assert_eq!(ok("img/x.png"), "C:/docs/report/img/x.png");
    assert_eq!(ok("./x.png"), "C:/docs/report/x.png");
    assert_eq!(ok("../shared/x.png"), "C:/docs/shared/x.png");
    assert_eq!(ok(r"\x.png"), "C:/x.png");
    assert_eq!(ok("C:x.png"), "C:/docs/report/x.png");
    assert_eq!(ok("D:x.png"), "D:x.png");
    assert_eq!(ok(r"D:\x.png"), "D:/x.png");
    assert_eq!(resolve(r"..\..\..\x.png", Some(doc)), Err(Refused::Invalid));

    let unix = Path::new("/home/u/docs/a.doc");
    assert_eq!(slashed(&resolve("img/x.png", Some(unix)).unwrap()), "/home/u/docs/img/x.png");
    assert_eq!(slashed(&resolve(r"img\x.png", Some(unix)).unwrap()), "/home/u/docs/img/x.png");
    assert_eq!(slashed(&resolve("../x.png", Some(unix)).unwrap()), "/home/u/x.png");

    let verbatim = Path::new(r"\\?\C:\docs\a.doc");
    assert_eq!(resolve("img/../x.png", Some(verbatim)).unwrap(), Path::new(r"\\?\C:\docs\x.png"));

    assert_eq!(ok(r"C:..\x.png"), "C:/docs/x.png");
    assert_eq!(ok(r"C:..\..\x.png"), "C:/x.png");
    assert_eq!(resolve(r"C:..\..\x.png", Some(Path::new(r"C:\docs\a.doc"))), Err(Refused::Invalid));
    assert_eq!(ok(r"./\\host\share\x.png"), "C:/docs/report/host/share/x.png");
    assert_eq!(ok(r"C:\\host\share"), "C:/host/share");
    assert_eq!(resolve("chapter1:intro.png", Some(doc)), Err(Refused::Url));
    assert_eq!(resolve("img/fig:1.png", Some(doc)), Err(Refused::Invalid));
    // The document's own name may contain colons.
    let colon_doc = Path::new("/home/u/a:b/c:d.doc");
    assert_eq!(slashed(&resolve("x.png", Some(colon_doc)).unwrap()), "/home/u/a:b/x.png");

    assert_eq!(slashed(&resolve("img/x.png", None).unwrap()), "img/x.png");
    assert_eq!(slashed(&resolve("../x.png", None).unwrap()), "../x.png");
}

#[test]
fn resolve_allows_only_the_document_share() {
    let doc = Path::new(r"\\Server\Share\docs\a.doc");
    assert_eq!(resolve(r"img\x.png", Some(doc)).unwrap(), Path::new(r"\\Server\Share\docs\img\x.png"));
    assert_eq!(resolve(r"\x.png", Some(doc)).unwrap(), Path::new(r"\\Server\Share\x.png"));
    assert_eq!(resolve(r"\\server\share\img\x.png", Some(doc)).unwrap(), Path::new(r"\\server\share\img\x.png"));
    assert!(resolve("//SERVER/SHARE/img/x.png", Some(doc)).is_ok());
    assert!(resolve(r"\\?\UNC\server\share\x.png", Some(doc)).is_ok());
    assert!(resolve("file://localhost//server/share/x.png", Some(doc)).is_ok());

    assert_eq!(resolve(r"\\other\share\x.png", Some(doc)), Err(Refused::Network));
    assert_eq!(resolve(r"\\server\other\x.png", Some(doc)), Err(Refused::Network));
    assert_eq!(resolve(r"\\server\share\..\other\x.png", Some(doc)), Err(Refused::Invalid));
    assert_eq!(resolve(r"..\..\..\x.png", Some(doc)), Err(Refused::Invalid));
    assert_eq!(resolve("file://server/share/x.png", Some(doc)), Err(Refused::Network));
    assert_eq!(resolve(r"\\server\share\nul", Some(doc)), Err(Refused::Device));

    let verbatim = Path::new(r"\\?\UNC\server\share\docs\a.doc");
    assert_eq!(resolve("img/x.png", Some(verbatim)).unwrap(), Path::new(r"\\?\UNC\server\share\docs\img\x.png"));
    assert!(resolve(r"\\server\share\x.png", Some(verbatim)).is_ok());

    let local = Path::new(r"C:\docs\a.doc");
    assert_eq!(resolve(r"\\server\share\x.png", Some(local)), Err(Refused::Network));
    assert_eq!(resolve(r"\\server\share\x.png", None), Err(Refused::Network));
}

#[test]
fn resolve_file_urls() {
    let doc = Path::new(r"C:\docs\a.doc");
    let ok = |raw: &str| slashed(&resolve(raw, Some(doc)).unwrap());
    assert_eq!(ok("file:///D:/My%20Pictures/x.png"), "D:/My Pictures/x.png");
    assert_eq!(ok("file:///D:/a%23b.png#frag"), "D:/a#b.png");
    assert_eq!(ok("file:img/x.png"), "C:/docs/img/x.png");
    assert_eq!(slashed(&resolve("file:///home/u/x.png", None).unwrap()), "/home/u/x.png");
    assert_eq!(ok("file://C:/x.png"), "C:/x.png");
    assert_eq!(ok("file://d|/x.png"), "d:/x.png");
    assert_eq!(ok("file:///C:/caf%E9.png"), "C:/café.png");
    assert_eq!(ok("file:///C:/caf%C3%A9.png"), "C:/café.png");
    assert_eq!(ok("file:///C:/caf%E9/na%C3%AFve%80.png"), "C:/café/naïve€.png");
    assert_eq!(resolve("file://localhost", Some(doc)), Err(Refused::Invalid));
    assert_eq!(resolve("file://", Some(doc)), Err(Refused::Invalid));

    assert_eq!(resolve("file://host/s/x.png", Some(doc)), Err(Refused::Network));
    assert_eq!(resolve("file:%5C%5Chost%5Cs%5Cx.png", Some(doc)), Err(Refused::Network));
    assert_eq!(resolve("file:///%5C%5Chost/s/x.png", Some(doc)), Err(Refused::Network));
    assert_eq!(resolve("file:////./pipe/x", Some(doc)), Err(Refused::Device));
    assert_eq!(resolve("https://host/x.png", Some(doc)), Err(Refused::Url));
}

#[test]
fn resolve_refuses_device_documents_and_paths() {
    let doc = Path::new(r"\\?\GLOBALROOT\Device\X\a.doc");
    assert_eq!(resolve("x.png", Some(doc)), Err(Refused::Device));
    assert_eq!(resolve(r"C:\x.png", Some(doc)).unwrap(), Path::new(r"C:\x.png"));
    assert_eq!(resolve(r"\\.\pipe\x", Some(Path::new(r"C:\a.doc"))), Err(Refused::Device));
}

#[test]
fn resolve_absolute_refuses_results_that_are_not_absolute() {
    for raw in ["img/x.png", "../x.png", "x.png", "file:img/x.png"] {
        assert!(resolve(raw, None).is_ok(), "{raw:?}");
        assert_eq!(resolve_absolute(raw, None), Err(Refused::Invalid), "{raw:?}");
    }
    assert_eq!(resolve_absolute(r"\\h\s\x.png", None), Err(Refused::Network));
    assert_eq!(resolve_absolute("https://h/x.png", None), Err(Refused::Url));

    let unix = Path::new("/home/u/docs/a.doc");
    let ok = |raw: &str, doc: Option<&Path>| slashed(&resolve_absolute(raw, doc).unwrap());
    assert_eq!(ok("img/x.png", Some(unix)), "/home/u/docs/img/x.png");
    assert_eq!(ok("../x.png", Some(unix)), "/home/u/x.png");
    assert_eq!(ok("file:///home/u/x.png", None), "/home/u/x.png");

    let win = Path::new(r"C:\docs\a.doc");
    if cfg!(windows) {
        assert_eq!(ok("img/x.png", Some(win)), "C:/docs/img/x.png");
        assert_eq!(ok(r"C:\x.png", None), "C:/x.png");
        assert_eq!(ok("file:///C:/x.png", None), "C:/x.png");
        assert_eq!(ok(r"\\?\C:\x.png", None), "//?/C:/x.png");
        assert_eq!(resolve_absolute(r"\x.png", None), Err(Refused::Invalid));
        assert_eq!(resolve_absolute("C:x.png", None), Err(Refused::Invalid));
        assert_eq!(resolve_absolute("D:x.png", Some(win)), Err(Refused::Invalid));
    } else {
        assert_eq!(ok("/x.png", None), "/x.png");
        assert_eq!(resolve_absolute("img/x.png", Some(win)), Err(Refused::Invalid));
        assert_eq!(resolve_absolute(r"C:\x.png", None), Err(Refused::Invalid));
    }
}

#[test]
fn refusals_have_messages() {
    for r in [Refused::Network, Refused::Device, Refused::Url, Refused::Invalid] {
        assert!(!r.to_string().is_empty());
    }
}
