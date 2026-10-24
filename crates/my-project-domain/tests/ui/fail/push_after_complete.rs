use my_project_domain::{ByteClass, BytePos, Line};

fn main() {
    let line = Line::starting_at(BytePos::ZERO).complete();
    let _ = line.push(ByteClass::Other);
}
