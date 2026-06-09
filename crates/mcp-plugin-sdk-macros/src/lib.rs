use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemMod};

#[proc_macro_attribute]
pub fn tool(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn tool_plugin(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let module = parse_macro_input!(item as ItemMod);
    let module_ident = module.ident;

    let mut tool_fns = Vec::new();
    let mut others: Vec<syn::Item> = Vec::new();

    if let Some((_, items)) = &module.content {
        for item in items {
            let is_tool = match item {
                syn::Item::Fn(func) => func.attrs.iter().any(|a| a.path().is_ident("tool")),
                _ => false,
            };

            if is_tool {
                if let syn::Item::Fn(func) = item {
                    let fn_name = &func.sig.ident;
                    let fn_name_str = fn_name.to_string();

                    let args_type = match func.sig.inputs.first() {
                        Some(syn::FnArg::Typed(pat)) => &pat.ty,
                        _ => {
                            return syn::Error::new_spanned(
                                &func.sig,
                                "#[tool] function must take exactly one argument (the args struct)",
                            )
                            .to_compile_error()
                            .into();
                        }
                    };

                    let description = extract_doc_comment(&func.attrs);

                    tool_fns.push((fn_name.clone(), fn_name_str, args_type.clone(), description));

                    // Include the function (without #[tool] attribute) in the module output
                    let mut func_no_attr = func.clone();
                    func_no_attr.attrs.retain(|a| !a.path().is_ident("tool"));
                    others.push(syn::Item::Fn(func_no_attr));
                }
            } else {
                others.push(item.clone());
            }
        }
    }

    // Build tool dispatch arms
    let mut call_arms = Vec::new();
    let mut tool_defs = Vec::new();

    for (_, fn_name_str, args_type, description) in &tool_fns {
        let fn_ident: syn::Ident = syn::Ident::new(fn_name_str, proc_macro2::Span::call_site());

        tool_defs.push(quote! {
            {
                let schema = mcp_plugin_sdk::schemars::schema_for!(#args_type);
                mcp_plugin_sdk::types::ToolDescriptor {
                    name: #fn_name_str.to_string(),
                    description: #description.to_string(),
                    input_schema: mcp_plugin_sdk::serde_json::to_value(&schema).unwrap(),
                }
            }
        });

        call_arms.push(quote! {
            #fn_name_str => {
                let args: #args_type = match mcp_plugin_sdk::serde_json::from_str(&args_str) {
                    Ok(a) => a,
                    Err(e) => {
                        return mcp_plugin_sdk::types::string_to_c_str(
                            mcp_plugin_sdk::serde_json::to_string(
                                &mcp_plugin_sdk::types::ToolResult::error(
                                    format!("Invalid arguments: {e}")
                                )
                            ).unwrap()
                        );
                    }
                };
                match #fn_ident(args) {
                    Ok(text) => mcp_plugin_sdk::types::ToolResult::success(text),
                    Err(text) => mcp_plugin_sdk::types::ToolResult::error(text),
                }
            }
        });
    }

    let expanded = quote! {
        mod #module_ident {
            use super::*;

            #(#others)*

            #[no_mangle]
            pub extern "C" fn plugin_abi_version() -> u32 {
                mcp_plugin_sdk::types::ABI_VERSION
            }

            #[no_mangle]
            pub extern "C" fn plugin_name() -> *mut std::os::raw::c_char {
                mcp_plugin_sdk::types::string_to_c_str(
                    stringify!(#module_ident).to_string()
                )
            }

            #[no_mangle]
            pub extern "C" fn plugin_list_tools() -> *mut std::os::raw::c_char {
                let tools: Vec<mcp_plugin_sdk::types::ToolDescriptor> = vec![
                    #(#tool_defs),*
                ];
                let json = mcp_plugin_sdk::serde_json::to_string(&tools).unwrap();
                mcp_plugin_sdk::types::string_to_c_str(json)
            }

            #[no_mangle]
            pub extern "C" fn plugin_call_tool(
                name: *const std::os::raw::c_char,
                args_json: *const std::os::raw::c_char,
            ) -> *mut std::os::raw::c_char {
                let name_str = unsafe { mcp_plugin_sdk::types::c_str_to_string(name) };
                let args_str = unsafe { mcp_plugin_sdk::types::c_str_to_string(args_json) };

                let result = match name_str.as_str() {
                    #(#call_arms)*
                    _ => mcp_plugin_sdk::types::ToolResult::error(format!("Unknown tool: {name_str}")),
                };

                let json = mcp_plugin_sdk::serde_json::to_string(&result).unwrap();
                mcp_plugin_sdk::types::string_to_c_str(json)
            }

            #[no_mangle]
            pub extern "C" fn plugin_free_string(s: *mut std::os::raw::c_char) {
                unsafe { mcp_plugin_sdk::types::free_c_str(s) }
            }
        }
    };

    expanded.into()
}

fn extract_doc_comment(attrs: &[syn::Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let syn::Meta::NameValue(nv) = &attr.meta {
                if let syn::Expr::Lit(lit) = &nv.value {
                    if let syn::Lit::Str(s) = &lit.lit {
                        lines.push(s.value().trim().to_string());
                    }
                }
            }
        }
    }
    lines.join(" ")
}
