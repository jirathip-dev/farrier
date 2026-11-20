use std::fs;
use std::io;
use std::path::Path;

use crate::files::FILES;

pub fn write(to: &Path) -> io::Result<()> {
    let root = to.join("farrier");
    for (name, text) in FILES {
        let path = root.join(name);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, text)?;
    }
    Ok(())
}
