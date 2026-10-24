use my_project_domain::{Elapsed, Millis};

fn main() {
    let _: Elapsed = Millis::from_host(5).since(Millis::ZERO);
    let _ = Millis::ZERO;
}
