/*
#[link(wasm_import_module = "snek_os")]
extern "C" {
    #[link_name = "test"]
    fn host_test(a: i32) -> ();
}
*/

fn main() {
    println!("{}", 42);
}
