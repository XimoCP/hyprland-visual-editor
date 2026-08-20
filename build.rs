fn main() {
    // ui/main.slint imports shell.slint and tokens.slint transitively; the
    // slint-build compiler resolves the whole tree from this single entry
    // point. Delivery 4 (hve2-trunk) mounts ShellRoot inside MainWindow,
    // so the standalone shell.slint compile from delivery 3 is no longer
    // needed — dropping it also removes the "ShellRoot doesn't inherit
    // Window" deprecation warning (the component is now used inside a
    // Window, not exported standalone).
    slint_build::compile("ui/main.slint").unwrap();
}
