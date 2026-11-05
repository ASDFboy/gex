// The library side: loading files, calling their functions, host commands.

use std::io::Write;
use std::sync::{Arc, Mutex};

use jlb::embed::{as_f64, fault_report, named_table, num, table_get};
use jlb::host::{self, Host};
use jlb::{Interp, Value, R};

struct Shared(Arc<Mutex<Vec<u8>>>);
impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Game {
    spawned: Arc<Mutex<Vec<String>>>,
}
impl Host for Game {
    fn call(&mut self, _it: &mut Interp, name: &'static str, a: Vec<Value>) -> Result<R<Value>, Vec<Value>> {
        match name {
            "SPAWN" => {
                self.spawned.lock().unwrap().push(format!("{}", jlb::value::display(&a[0])));
                Ok(Ok(num(7.0)))
            }
            "KEY_DOWN" => Ok(Ok(Value::Bool(true))),
            _ => Err(a),
        }
    }
}

#[test]
fn modules_hosts_and_calls() {
    host::register(&["SPAWN"]);
    let dir = std::env::temp_dir().join("jlb-embed-test");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("player.jlb");
    std::fs::write(
        &file,
        "hp = 10\nDEF on_step(dt) {\n    self.x += dt * 2\n    GLOBAL hp -= 1\n    IF KEY_DOWN(\"left\") { PRINT(\"left\") }\n    RETURN SPAWN(\"bullet\")\n}\n",
    )
    .unwrap();

    let out = Arc::new(Mutex::new(Vec::new()));
    let spawned = Arc::new(Mutex::new(Vec::new()));
    let mut it = Interp::embedded(dir.clone());
    it.set_output(Box::new(Shared(out.clone())));
    it.host = Some(Box::new(Game { spawned: spawned.clone() }));

    let me = named_table(vec![("x", num(1.0))]);
    let a = it.load_module(&file, &[("self", me.clone())]).unwrap();
    let b = it.load_module(&file, &[("self", named_table(vec![("x", num(0.0))]))]).unwrap();
    let f = Interp::env_get(&a, "on_step").unwrap();
    let r = it.call_value(&f, vec![num(0.5)]).unwrap();
    assert_eq!(as_f64(&r), Some(7.0));
    // The table given as `self` is shared, so the host sees the change.
    assert_eq!(as_f64(&table_get(&Interp::env_get(&a, "self").unwrap(), "x").unwrap()), Some(2.0));
    assert_eq!(as_f64(&Interp::env_get(&a, "hp").unwrap()), Some(9.0));
    assert_eq!(as_f64(&Interp::env_get(&b, "hp").unwrap()), Some(10.0), "each load has its own variables");
    assert_eq!(String::from_utf8(out.lock().unwrap().clone()).unwrap(), "left\n");
    assert_eq!(spawned.lock().unwrap().as_slice(), ["bullet"]);

    // Errors come back as reports with the file and line.
    std::fs::write(dir.join("bad.jlb"), "DEF on_step() {\n    x = 1 + TRUE\n}\n").unwrap();
    let e = it.load_module(&dir.join("bad.jlb"), &[]).unwrap();
    let f = Interp::env_get(&e, "on_step").unwrap();
    let report = fault_report(&it.call_value(&f, vec![]).unwrap_err()).unwrap();
    assert!(report.contains("bad.jlb, line 2"), "{}", report);

    // Stopping from outside.
    std::fs::write(dir.join("loop.jlb"), "DEF on_step() {\n    WHILE TRUE { }\n}\n").unwrap();
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    it.cancel = Some(flag.clone());
    let l = it.load_module(&dir.join("loop.jlb"), &[]).unwrap();
    flag.store(true, std::sync::atomic::Ordering::Relaxed);
    let f = Interp::env_get(&l, "on_step").unwrap();
    assert!(fault_report(&it.call_value(&f, vec![]).unwrap_err()).is_none());
}
