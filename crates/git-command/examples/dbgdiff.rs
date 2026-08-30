fn main() {
    let repo = git_core::Repository::discover_from(
        std::path::Path::new("/tmp/a8p"),
        &git_core::RepoEnv::default(),
    )
    .unwrap();
    let odb = git_odb::Odb::from_repo(&repo).unwrap();
    let head = git_revision::Resolver::new(&repo).unwrap().resolve("HEAD").unwrap();
    let obj = odb.read(&head).unwrap();
    let commit = git_object::parse_commit(&obj.data, repo.hash_algo).unwrap();
    let head_entries =
        git_object::parse_tree(&odb.read(&commit.tree).unwrap().data, repo.hash_algo).unwrap();
    for e in &head_entries {
        let name: String = e.name.iter().map(|b| *b as char).collect();
        println!("head: {name} {}", e.oid);
    }
    // Verify g.txt blob content
    let g = head_entries.iter().find(|e| e.name == b"g.txt").unwrap();
    let blob = odb.read(&g.oid).unwrap();
    println!("g.txt content: {:?}", String::from_utf8_lossy(&blob.data));
}
