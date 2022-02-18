## Introduction
We all love cargo, having an idea, do `cargo new`, type our idea and `cargo r`, 
easy right (*except for the compilation errors*)? 
Well... sometimes you want to do that in C or C++ but you have to create 
a Makefile, or use CMake or the cli, but in all these scenarios adding 
tests/examples/benchs is hard, so let's change that. <br>

The philosophy of this project is from something simple then be able to expand to more 
complex stuff. <br>

You want to just create a simple binary app? `amargo new myproject`. <br>
You want to create a dynamic library? `amargo new mylib --dylib`. <br>
Wanna test the stuff in the `tests` folder? `amargo test`. <br>
Just compile? `amargo build` or `amargo build release`. <br>
Install locally? `amargo install` <br>
 
## Get Started
For the moment a simple `cargo install --path .` should work
 
#### Platforms
- ✔️ Windows 7,8,10,11
- ✔️ Linux
- ⚠️ macOS (not tested, should work)
 
#### Objectives
- ✔️ Minimal functional state (create binary project and compile with release or debug)<br>
- ✔️ Support more compilers than clang<br>
- ❌ Support more types of crate (dynamic libs, static libs, header only) 
        **IN PROGRESS** <br>
- ⚠️ Don't recompile if isn't needed and compile just the needed sources<br>
    - ✔️ Incremental compilation for sources <br>
	- ✔️ Incremental compilation for headers <br>
    - ❌Incremental compilation for source that include source (maybe forbid this?)
- ❌ Have some sort of config file (maybe using toml)<br>
- ❌ Have an installation dir and command, like `.amargo/bin` and `.amargo/lib` <br>
- ❌ Support tests<br>
- ❌ Support C++<br>
- ❌ Maybe external dependencies? (using vcpkg or a custom dependency system)<br>
 
## Contribution
Pls help me, through the code there are a lot of TODOs if you wanna help, but pls 
comment a lot what you do and don't be afraid of creating new TODOs

