fn main() {
    // SQLx's embedded migration list must rebuild when a new file is added to the directory.
    println!("cargo:rerun-if-changed=../../migrations");
}
