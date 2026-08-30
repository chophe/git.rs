fn main() {
    let ix = git_index::Index::read(std::path::Path::new("/tmp/a8p/.git/index"), git_hash::HashAlgorithm::Sha1);
    match ix {
        Ok(i) => {
            println!("entries: {}", i.entries.len());
            for e in &i.entries {
                println!("  {} mode={:o} size={}", e.name, e.mode, e.size);
            }
        }
        Err(e) => println!("ERR: {e:?}"),
    }
}
