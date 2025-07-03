#[cfg(test)]
pub struct TestFS {
    root: vfs::VfsPath,
}

#[cfg(test)]
impl TestFS {
    pub fn new<I, P, C>(entries: I) -> Self
    where
        I: IntoIterator<Item = (P, C)>,
        P: AsRef<str>,
        C: AsRef<[u8]>,
    {
        let fs = vfs::MemoryFS::new();
        let root: vfs::VfsPath = fs.into();
        for (path, content) in entries {
            let p = root.join(path.as_ref()).unwrap();
            let parent = p.parent();
            if parent.as_str() != p.as_str() {
                parent.create_dir_all().unwrap();
            }
            let mut f = p.create_file().unwrap();
            use std::io::Write;
            f.write_all(content.as_ref()).unwrap();
        }
        Self { root }
    }

    pub fn root(&self) -> vfs::VfsPath {
        self.root.clone()
    }
}
