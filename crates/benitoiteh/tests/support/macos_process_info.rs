//! KERN_PROCARGS2 の正負の対照を本物のプロセスで作る（設計書 03-02「方針の要素」）。

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

const SCRIPT: &str = r#"import ctypes, errno, os, subprocess, sys

if sys.argv[1] == 'decoy':
    print(os.getpid(), flush=True)
    sys.stdin.readline()
    sys.exit(0)
if sys.argv[1] == 'launch':
    sys.exit(subprocess.call(sys.argv[2:], close_fds=True))

libc = ctypes.CDLL(None, use_errno=True)
libc.sysctl.argtypes = [ctypes.POINTER(ctypes.c_int), ctypes.c_uint,
                       ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t),
                       ctypes.c_void_p, ctypes.c_size_t]
libc.sysctl.restype = ctypes.c_int

def read(pid):
    mib = (ctypes.c_int * 3)(1, 49, pid)  # CTL_KERN, KERN_PROCARGS2
    buf = ctypes.create_string_buffer(1024 * 1024)
    size = ctypes.c_size_t(len(buf))
    if libc.sysctl(mib, 3, buf, ctypes.byref(size), None, 0) != 0:
        return ctypes.get_errno(), b''
    return 0, buf.raw[:size.value]

own_error, own = read(os.getpid())
assert own_error == 0 and b'procargs.py' in own, 'self query failed'
for label, pid, dummy in [('parent', os.getppid(), b'PROCARGS_PARENT_DUMMY=parent-dummy-value'),
                          ('outside', int(sys.argv[2]), b'PROCARGS_OUTSIDE_DUMMY=outside-dummy-value')]:
    error, value = read(pid)
    if sys.argv[1] == 'allowed':
        assert error == 0 and dummy in value, label + ' positive control failed'
    else:
        assert error == errno.EPERM and dummy not in value, label + ' environment was readable'
print('process-info: ' + sys.argv[1])
"#;

pub struct ProcessInfoProbe {
    python: PathBuf,
    script: PathBuf,
    root: PathBuf,
    decoy: Child,
}

impl ProcessInfoProbe {
    pub fn new(root: &Path) -> Result<Self, Box<dyn Error>> {
        for relative in ["home", "config", "state", "tmp"] {
            fs::create_dir_all(root.join(relative))?;
        }
        // Apple の起動用 shim は避け、サンドボックスへ実体のパスを渡す。
        let output = Command::new("/usr/bin/python3")
            .args([
                "-I",
                "-c",
                "import os, sys; print(os.path.realpath(sys.executable))",
            ])
            .env_clear()
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("TMPDIR", root.join("tmp"))
            .output()?;
        if !output.status.success() {
            return Err("Could not resolve Python for process-info probe".into());
        }
        let python = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        let script = root.join("procargs.py");
        fs::write(&script, SCRIPT)?;
        let mut decoy = Command::new(&python)
            .arg("-I")
            .arg(&script)
            .arg("decoy")
            .env_clear()
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("TMPDIR", root.join("tmp"))
            .env("PROCARGS_OUTSIDE_DUMMY", "outside-dummy-value")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let ready = decoy.stdout.take().ok_or("Missing decoy readiness pipe")?;
        let probe = Self {
            python,
            script,
            root: root.to_owned(),
            decoy,
        };
        let mut line = String::new();
        BufReader::new(ready).read_line(&mut line)?;
        if line.trim().parse::<u32>()? != probe.decoy.id() {
            return Err("Wrong decoy PID".into());
        }
        let mut direct = Command::new(&probe.python);
        direct.arg("-I").args(probe.args("allowed"));
        let output = probe.wrap(&direct).output()?;
        if !output.status.success() {
            return Err(format!("Process-info positive control failed: {output:?}").into());
        }
        Ok(probe)
    }

    pub fn python(&self) -> &Path {
        &self.python
    }

    pub fn args(&self, expected: &str) -> Vec<OsString> {
        vec![
            self.script.as_os_str().to_owned(),
            expected.into(),
            self.decoy.id().to_string().into(),
        ]
    }

    pub fn wrap(&self, command: &Command) -> Command {
        let mut wrapper = Command::new(&self.python);
        wrapper
            .arg("-I")
            .arg(&self.script)
            .arg("launch")
            .arg(command.get_program())
            .args(command.get_args())
            .env_clear()
            .envs(
                command
                    .get_envs()
                    .filter_map(|(key, value)| value.map(|value| (key, value))),
            )
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("PROCARGS_PARENT_DUMMY", "parent-dummy-value")
            .stdin(Stdio::null());
        if let Some(cwd) = command.get_current_dir() {
            wrapper.current_dir(cwd);
        }
        wrapper
    }
}

impl Drop for ProcessInfoProbe {
    fn drop(&mut self) {
        drop(self.decoy.stdin.take());
        let _result = self.decoy.wait();
    }
}
