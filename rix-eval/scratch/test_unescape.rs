use rix_parser::ast::str_util::unescape;

fn main() {
    let s = "foo\\nbar";
    let u = unescape(s, false);
    println!("input: {}", s);
    println!("output: {}", u);
    println!("output len: {}", u.len());
}
