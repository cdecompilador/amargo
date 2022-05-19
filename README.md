# amargo
The Rust Cargo tool but for your C or C++ projects

We all love [cargo](https://github.com/rust-lang/cargo), having an idea, do `cargo new`, type our idea and `cargo run`, easy right (*except for the compilation errors*)? \
Well... sometimes you want to do that in C or C++ but you have to create 
a Makefile, or use CMake or the cli, but in all these scenarios adding "tests/examples/benchs" is hard, so let's change that.

The philosophy of this project is from something simple then be able to expand to more complex stuff.
 
## Get Started
To install the tool, clone this repository and run `cargo install --path .`, it should work ! (This solution is by waiting pushing real releases on this Github repository)
 
- Create a simple binary app : `amargo new <my_app>`
- Create a dynamic library : `amargo new <my_lib> --dylib`
- Test stuff in "tests/" : `amargo test`
- Common build command : `amargo build` or `amargo release`
- Install locally : `amargo install`

## Available Platforms
- ✔️ Windows 7,8,10,11
- ✔️ Linux
- ⚠️ macOS (not tested, should work)
 
## Objectives
- ✔️ Minimal functional state (create binary project and compile with release or debug)<br>
- ✔️ Support more compilers than clang<br>
- ❌ Support more types of crate (dynamic libs, static libs, header only) 
        **IN PROGRESS** <br>
- ⚠️ Don't recompile if isn't needed and compile just the needed sources<br>
    - ✔️ Incremental compilation for sources <br>
	- ✔️ Incremental compilation for headers <br>
    - ❌ Incremental compilation for source that include source (maybe forbid this?)
- ❌ Have some sort of config file (maybe using toml)<br>
- ❌ Have an installation dir and command, like `.amargo/bin` and `.amargo/lib` <br>
- ❌ Support tests<br>
- ❌ Support C++<br>
- ❌ Maybe external dependencies? (using vcpkg or a custom dependency system)<br>
 
## Contribution
Please help me, through the code there are a lot of TODOs if you wanna help, but please comment a lot what you do and don't be afraid of creating new TODOs.
