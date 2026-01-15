// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

// use mlua::{Lua, UserData};


// impl UserData for Lua {
//     fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
//         // Implementation
//     }
// }

// #[derive(Default)]
// pub struct WriteStream {
//     chunks: Vec<String>,
// }

// impl WriteStream {
//     pub fn write(&mut self, s: &str) {
//         self.chunks.push(s.to_owned());
//     }

//     pub fn render(&self) -> String {
//         self.chunks.concat()
//     }
// }

// impl mlua::UserData for WriteStream {
//     fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
//         methods.add_method_mut("write", |_, this, s: String| {
//             this.write(&s);
//             Ok(())
//         });

//         methods.add_method("render", |_, this, ()| {
//             Ok(this.render())
//         });
//     }
// }
// pub fn write_stream_module(lua: &Lua) -> mlua::Result<Table> {
//     let module = lua.create_table()?;
//     let constructor = lua.create_function(|_, ()| {
//         Ok(WriteStream::default())
//     })?;
//     module.set("new", constructor)?;
//     Ok(module)
// }