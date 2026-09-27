//! T525 native executable fixture. No host shell, real devices or ADB server.
use std::{fs, io::Read, path::Path};
fn main() {
    let root = std::env::current_exe().unwrap().parent().unwrap().to_owned();
    let args: Vec<_> = std::env::args().skip(1).collect();
    fs::write(root.join("invoked"), args.join("\n")).unwrap();
    if args == ["devices"] {
        print!("{}", fs::read_to_string(root.join("inventory")).unwrap());
        return;
    }
    assert_eq!(&args[..2], ["-s", "USB"]);
    match args[2].as_str() {
        "reverse" => reverse(&root, &args[3..]),
        "shell" if args[3] == "pm" => println!("package:/data/app/blent/base.apk"),
        "shell" => {
            assert_eq!(args[3], "-T");
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();
            fs::write(root.join("delivered"), input).unwrap();
        }
        _ => panic!("unexpected command: {args:?}"),
    }
}
fn reverse(root: &Path, args: &[String]) {
    let path = root.join("routes");
    let mut routes = fs::read_to_string(&path).unwrap_or_default();
    match args[0].as_str() {
        "--list" => { print!("{routes}"); return; }
        "--no-rebind" => {
            assert!(!routes.lines().any(|line| line.split_whitespace().nth(1) == Some(&args[1])));
            routes.push_str(&format!("UsbFfs {} {}\n", args[1], args[2]));
        }
        "--remove" => {
            routes = routes.lines().filter(|line| line.split_whitespace().nth(1) != Some(&args[1]))
                .map(|line| format!("{line}\n")).collect();
        }
        _ => panic!("unexpected reverse command: {args:?}"),
    }
    fs::write(path, routes).unwrap();
}
