// Disposable real /proc fixture. Only children created here are ever signalled.
use std::io::BufRead;

pub struct Fixture {
    pub root: tempfile::TempDir,
    program: std::path::PathBuf,
}

pub struct Child(std::process::Child);
impl Child {
    pub fn pid(&self) -> u32 { self.0.id() }
}
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("daemon.c");
        let program = root.path().join("uscreen");
        std::fs::write(&source, "#include <stdio.h>\n#include <unistd.h>\nint main(void) { puts(\"ready\"); fflush(stdout); for (;;) pause(); }\n").unwrap();
        assert!(std::process::Command::new("cc").arg(source).arg("-o").arg(&program).status().unwrap().success());
        Self { root, program }
    }

    pub fn start(&self, args: &[&str]) -> Child {
        let mut child = Child(std::process::Command::new(&self.program).args(args)
            .stdout(std::process::Stdio::piped()).spawn().unwrap());
        let mut line = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap()).read_line(&mut line).unwrap();
        assert_eq!(line, "ready\n");
        child
    }
}
