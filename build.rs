// 构建元数据：VERGEN_BUILD_* / VERGEN_GIT_*（调 git CLI，§2 禁 git2）。
// git 不可用时 vergen 默认 fail_on_error=false，降级为 "unknown" 并出 cargo:warning。
use vergen_gitcl::{Build, Emitter, Gitcl};

fn main() {
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let build = Build::all_build();
        let gitcl = Gitcl::all_git();
        Emitter::default()
            .add_instructions(&build)?
            .add_instructions(&gitcl)?
            .emit()?;
        Ok(())
    })();
    if let Err(e) = result {
        println!("cargo:warning=winterjs: build metadata unavailable: {e}");
    }
}
