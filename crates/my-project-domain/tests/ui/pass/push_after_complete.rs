use my_project_domain::{ByteClass, BytePos, Line};

fn main() {
    let mut line = Line::starting_at(BytePos::ZERO);
    let _ = line.push(ByteClass::Other);
}
