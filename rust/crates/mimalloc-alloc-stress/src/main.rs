fn main() {
    let rc = mimalloc_alloc_stress::run();
    if rc == 0 {
        println!("alloc-stress ok");
    } else {
        eprintln!("alloc-stress probe {rc}");
    }
    std::process::exit(rc);
}
