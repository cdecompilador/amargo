## Introduction
We all love cargo, having an idea, do `cargo new`, type our idea and `cargo r`, easy right?
Well... sometimes you want to do that in C or C++ but you have to create a Makefile, or use CMake or
the cli, but in all these scenarios adding tests/examples/benchs is hard. The philosophy is from something simple
then be able to expand to more complex stuff. You want to just create a simple binary app? `amargo new myproject`.
You want to create a dynamic library? `amargo new mylib --dylib`. Wanna test the stuff in the `tests` folder?
`amargo test`. Just compile? `amargo build` or `amargo build release`. 
 
## Get Started
For the moment a simple `cargo b` should work
 
#### Platforms
- ✔️ Windows 7,8,10,11
- ⚠️ Linux (not tested)
- ⚠️ macOS (not tested)
 
#### Objectives
- ✔️ Minimal functional state (create binary project and compile with release or debug)<br>
- ✔️ Support more compilers than clang<br>
- ❌ Support more types of crate (dynamic libs, static libs, header only)<br>
- ⚠️ Don't recompile if isn't needed and compile just the needed sources<br>
    - ✔️ Incremental compilation for sources<br>
	- ❌ Incremental compilation for headers (needs parsing)<br>
- ❌ Have some sort of config file (maybe using toml)<br>
- ❌ Support tests<br>
- ❌ Support benchmarks<br>
- ❌ Maybe external dependencies? (using vcpkg or a custom dependency system)<br>
 
## Contribution
Pls help me, through the code there are a lot of TODOs if you wanna help, but pls comment a lot what
you do and don't be afraid of creating new TODOs

