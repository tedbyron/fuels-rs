use std::collections::HashMap;

use crate::code_gen::bindings::ContractBindings;
use crate::code_gen::custom_types_gen::{
    expand_custom_enum, expand_custom_struct, extract_custom_type_name_from_abi_property,
};
use crate::code_gen::functions_gen::expand_function;
use crate::errors::Error;
use crate::json_abi::ABIParser;
use crate::source::Source;
use crate::utils::ident;
use fuels_types::{JsonABI, Property};

use crate::constants::{ENUM_KEYWORD, STRUCT_KEYWORD};
use proc_macro2::{Ident, TokenStream};
use quote::quote;

pub struct Abigen {
    /// The parsed ABI.
    abi: JsonABI,

    /// The parser used to transform the JSON format into `JsonABI`
    abi_parser: ABIParser,

    /// The contract name as an identifier.
    contract_name: Ident,

    custom_structs: HashMap<String, Property>,

    custom_enums: HashMap<String, Property>,

    /// Format the code using a locally installed copy of `rustfmt`.
    rustfmt: bool,

    /// Generate no-std safe code
    no_std: bool,
}

pub fn is_custom_type(p: &Property) -> bool {
    p.type_field.contains(ENUM_KEYWORD) || p.type_field.contains(STRUCT_KEYWORD)
}

impl Abigen {
    /// Creates a new contract with the given ABI JSON source.
    pub fn new<S: AsRef<str>>(contract_name: &str, abi_source: S) -> Result<Self, Error> {
        let source = Source::parse(abi_source).unwrap();
        let mut parsed_abi: JsonABI = serde_json::from_str(&source.get().unwrap())?;

        // Filter out outputs with empty returns. These are
        // generated by forc's json abi as `"name": ""` and `"type": "()"`
        for f in &mut parsed_abi {
            let index = f
                .outputs
                .iter()
                .position(|p| p.name.is_empty() && p.type_field == "()");

            match index {
                Some(i) => f.outputs.remove(i),
                None => continue,
            };
        }
        let custom_types = Abigen::get_custom_types(&parsed_abi);
        Ok(Self {
            custom_structs: custom_types
                .clone()
                .into_iter()
                .filter(|(_, p)| p.type_field.contains(STRUCT_KEYWORD))
                .collect(),
            custom_enums: custom_types
                .into_iter()
                .filter(|(_, p)| p.type_field.contains(ENUM_KEYWORD))
                .collect(),
            abi: parsed_abi,
            contract_name: ident(contract_name),
            abi_parser: ABIParser::new(),
            rustfmt: true,
            no_std: false,
        })
    }

    pub fn no_std(mut self) -> Self {
        self.no_std = true;
        self
    }

    /// Generates the contract bindings.
    pub fn generate(self) -> Result<ContractBindings, Error> {
        let rustfmt = self.rustfmt;
        let tokens = self.expand()?;

        Ok(ContractBindings { tokens, rustfmt })
    }

    /// Entry point of the Abigen's expansion logic.
    /// The high-level goal of this function is to expand* a contract
    /// defined as a JSON into type-safe bindings of that contract that can be
    /// used after it is brought into scope after a successful generation.
    ///
    /// *: To expand, in procedural macro terms, means to automatically generate
    /// Rust code after a transformation of `TokenStream` to another
    /// set of `TokenStream`. This generated Rust code is the brought into scope
    /// after it is called through a procedural macro (`abigen!()` in our case).
    pub fn expand(&self) -> Result<TokenStream, Error> {
        let name = &self.contract_name;
        let name_mod = ident(&format!(
            "{}_mod",
            self.contract_name.to_string().to_lowercase()
        ));

        let contract_functions = self.functions()?;
        let abi_structs = self.abi_structs()?;
        let abi_enums = self.abi_enums()?;

        let (includes, code) = if self.no_std {
            (
                quote! {
                    use alloc::{vec, vec::Vec};
                },
                quote! {},
            )
        } else {
            (
                quote! {
                    use fuel_tx::{ContractId, Address};
                    use fuels_rs::contract::contract::{Contract, ContractCall};
                    use fuels_rs::signers::{provider::Provider, LocalWallet};
                    use std::str::FromStr;
                },
                quote! {
                    pub struct #name {
                        contract_id: ContractId,
                        provider: Provider,
                        wallet: LocalWallet
                    }

                    impl #name {
                        pub fn new(contract_id: String, provider: Provider, wallet: LocalWallet)
                        -> Self {
                            let contract_id = ContractId::from_str(&contract_id).unwrap();
                            Self{ contract_id, provider, wallet }
                        }
                        #contract_functions
                    }
                },
            )
        };

        Ok(quote! {
            pub use #name_mod::*;

            #[allow(clippy::too_many_arguments)]
            mod #name_mod {
                #![allow(clippy::enum_variant_names)]
                #![allow(dead_code)]
                #![allow(unused_imports)]

                #includes
                use fuels_rs::core::{Detokenize, EnumSelector, ParamType, Tokenizable, Token};

                #code

                #abi_structs
                #abi_enums
            }
        })
    }

    pub fn functions(&self) -> Result<TokenStream, Error> {
        let mut tokenized_functions = Vec::new();

        for function in &self.abi {
            let tokenized_fn = expand_function(
                function,
                &self.abi_parser,
                &self.custom_enums,
                &self.custom_structs,
            )?;
            tokenized_functions.push(tokenized_fn);
        }

        Ok(quote! { #( #tokenized_functions )* })
    }

    fn abi_structs(&self) -> Result<TokenStream, Error> {
        let mut structs = TokenStream::new();

        // Prevent expanding the same struct more than once
        let mut seen_struct: Vec<&str> = vec![];

        for prop in self.custom_structs.values() {
            // Skip custom type generation if the custom type is a Sway-native type.
            // This means ABI methods receiving or returning a Sway-native type
            // can receive or return that native type directly.
            if prop.type_field.contains("ContractId") || prop.type_field.contains("Address") {
                continue;
            }

            if !seen_struct.contains(&prop.type_field.as_str()) {
                structs.extend(expand_custom_struct(prop)?);
                seen_struct.push(&prop.type_field);
            }
        }

        Ok(structs)
    }

    fn abi_enums(&self) -> Result<TokenStream, Error> {
        let mut enums = TokenStream::new();

        for (name, prop) in &self.custom_enums {
            enums.extend(expand_custom_enum(name, prop)?);
        }

        Ok(enums)
    }

    fn get_all_properties(abi: &JsonABI) -> Vec<&Property> {
        let mut all_properties: Vec<&Property> = vec![];
        for function in abi {
            for prop in &function.inputs {
                all_properties.push(prop);
            }
            for prop in &function.outputs {
                all_properties.push(prop);
            }
        }
        all_properties
    }

    /// Reads the parsed ABI and returns the custom types (either `struct` or `enum`) found in it.
    fn get_custom_types(abi: &JsonABI) -> HashMap<String, Property> {
        let mut custom_types = HashMap::new();
        let mut inner_custom_types: Vec<Property> = Vec::new();

        let all_properties = Abigen::get_all_properties(abi);

        for prop in all_properties {
            if is_custom_type(prop) {
                // Top level custom type
                let custom_type_name = extract_custom_type_name_from_abi_property(prop, None)
                    .expect("failed to extract custom type name");
                custom_types
                    .entry(custom_type_name)
                    .or_insert_with(|| prop.clone());

                // Find inner {structs, enums} in case of nested custom types
                for inner_component in prop.components.as_ref().unwrap() {
                    inner_custom_types.extend(Abigen::get_inner_custom_properties(inner_component));
                }
            }
        }

        for inner_custom_type in inner_custom_types {
            if is_custom_type(&inner_custom_type) {
                // A {struct, enum} can contain another {struct, enum}
                let inner_custom_type_name =
                    extract_custom_type_name_from_abi_property(&inner_custom_type, None)
                        .expect("failed to extract nested custom type name");
                custom_types
                    .entry(inner_custom_type_name)
                    .or_insert(inner_custom_type);
            }
        }

        custom_types
    }

    // Recursively gets inner properties defined in nested structs or nested enums
    fn get_inner_custom_properties(prop: &Property) -> Vec<Property> {
        let mut props = Vec::new();

        if is_custom_type(prop) {
            props.push(prop.clone());

            for inner_prop in prop.components.as_ref().unwrap() {
                let inner = Abigen::get_inner_custom_properties(inner_prop);
                props.extend(inner);
            }
        }

        props
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_bindings() {
        let contract = r#"
        [
            {
                "type":"contract",
                "inputs":[
                    {
                        "name":"arg",
                        "type":"u32"
                    }
                ],
                "name":"takes_u32_returns_bool",
                "outputs":[
                    {
                        "name":"",
                        "type":"bool"
                    }
                ]
            }
        ]
        "#;

        let _bindings = Abigen::new("test", contract).unwrap().generate().unwrap();
    }

    #[test]
    fn generates_bindings_two_args() {
        let contract = r#"
        [
            {
                "type":"contract",
                "inputs":[
                    {
                        "name":"arg",
                        "type":"u32"
                    },
                    {
                        "name":"second_arg",
                        "type":"u16"
                    }
                ],
                "name":"takes_ints_returns_bool",
                "outputs":[
                    {
                        "name":"",
                        "type":"bool"
                    }
                ]
            }
        ]
        "#;

        // We are expecting a MissingData error because at the moment, the
        // ABIgen expects exactly 4 arguments (see `expand_function_arguments`), here
        // there are 5
        let _bindings = Abigen::new("test", contract).unwrap().generate().unwrap();
    }

    #[test]
    fn custom_struct() {
        let contract = r#"
        [
            {
                "type":"contract",
                "inputs":[
                    {
                        "name":"value",
                        "type":"struct MyStruct",
                        "components": [
                            {
                                "name": "foo",
                                "type": "u8"
                            },
                            {
                                "name": "bar",
                                "type": "bool"
                            }
                        ]
                    }
                ],
                "name":"takes_struct",
                "outputs":[]
            }
        ]
        "#;

        let contract = Abigen::new("custom", contract).unwrap();

        assert_eq!(1, contract.custom_structs.len());

        assert!(contract.custom_structs.contains_key("MyStruct"));

        let _bindings = contract.generate().unwrap();
    }

    #[test]
    fn multiple_custom_types() {
        let contract = r#"
        [
            {
                "type":"contract",
                "inputs":[
                {
                    "name":"input",
                    "type":"struct MyNestedStruct",
                    "components":[
                    {
                        "name":"x",
                        "type":"u16"
                    },
                    {
                        "name":"foo",
                        "type":"struct InnerStruct",
                        "components":[
                        {
                            "name":"a",
                            "type":"bool"
                        },
                        {
                            "name":"b",
                            "type":"u8[2]"
                        }
                        ]
                    }
                    ]
                },
                {
                    "name":"y",
                    "type":"struct MySecondNestedStruct",
                    "components":[
                    {
                        "name":"x",
                        "type":"u16"
                    },
                    {
                        "name":"bar",
                        "type":"struct SecondInnerStruct",
                        "components":[
                        {
                            "name":"inner_bar",
                            "type":"struct ThirdInnerStruct",
                            "components":[
                            {
                                "name":"foo",
                                "type":"u8"
                            }
                            ]
                        }
                        ]
                    }
                    ]
                }
                ],
                "name":"takes_nested_struct",
                "outputs":[

                ]
            }
        ]
        "#;

        let contract = Abigen::new("custom", contract).unwrap();

        assert_eq!(5, contract.custom_structs.len());

        let expected_custom_struct_names = vec![
            "MyNestedStruct",
            "InnerStruct",
            "MySecondNestedStruct",
            "SecondInnerStruct",
            "ThirdInnerStruct",
        ];

        for name in expected_custom_struct_names {
            assert!(contract.custom_structs.contains_key(name));
        }
    }

    #[test]
    fn single_nested_struct() {
        let contract = r#"
        [
            {
                "type":"contract",
                "inputs":[
                    {
                        "name":"top_value",
                        "type":"struct MyNestedStruct",
                        "components": [
                            {
                                "name": "x",
                                "type": "u16"
                            },
                            {
                                "name": "foo",
                                "type": "struct InnerStruct",
                                "components": [
                                    {
                                        "name":"a",
                                        "type": "bool"
                                    }
                                ]
                            }
                        ]
                    }
                ],
                "name":"takes_nested_struct",
                "outputs":[]
            }
        ]
        "#;

        let contract = Abigen::new("custom", contract).unwrap();

        assert_eq!(2, contract.custom_structs.len());

        assert!(contract.custom_structs.contains_key("MyNestedStruct"));
        assert!(contract.custom_structs.contains_key("InnerStruct"));

        let _bindings = contract.generate().unwrap();
    }

    #[test]
    fn custom_enum() {
        let contract = r#"
        [
            {
                "type":"contract",
                "inputs":[
                    {
                        "name":"my_enum",
                        "type":"enum MyEnum",
                        "components": [
                            {
                                "name": "x",
                                "type": "u32"
                            },
                            {
                                "name": "y",
                                "type": "bool"
                            }
                        ]
                    }
                ],
                "name":"takes_enum",
                "outputs":[]
            }
        ]
        "#;

        let contract = Abigen::new("custom", contract).unwrap();

        assert_eq!(1, contract.custom_enums.len());
        assert_eq!(0, contract.custom_structs.len());

        assert!(contract.custom_enums.contains_key("MyEnum"));

        let _bindings = contract.generate().unwrap();
    }
    #[test]
    fn output_types() {
        let contract = r#"
        [
            {
                "type":"contract",
                "inputs":[
                    {
                        "name":"value",
                        "type":"struct MyStruct",
                        "components": [
                            {
                                "name": "a",
                                "type": "str[4]"
                            },
                            {
                                "name": "foo",
                                "type": "[u8; 2]"
                            },
                            {
                                "name": "bar",
                                "type": "bool"
                            }
                        ]
                    }
                ],
                "name":"takes_enum",
                "outputs":[
                    {
                        "name":"ret",
                        "type":"struct MyStruct",
                        "components": [
                            {
                                "name": "a",
                                "type": "str[4]"
                            },
                            {
                                "name": "foo",
                                "type": "[u8; 2]"
                            },
                            {
                                "name": "bar",
                                "type": "bool"
                            }
                        ]
                    }
                ]
            }
        ]
        "#;

        let contract = Abigen::new("custom", contract).unwrap();
        let _bindings = contract.generate().unwrap();
    }
    #[test]
    fn test_abigen_struct_inside_enum() {
        let contract = r#"
[
  {
    "type": "function",
    "inputs": [
      {
        "name": "b",
        "type": "enum Bar",
        "components": [
          {
            "name": "waiter",
            "type": "struct Waiter",
            "components": [
              {
                "name": "name",
                "type": "u8",
                "components": null
              },
              {
                "name": "male",
                "type": "bool",
                "components": null
              }
            ]
          },
          {
            "name": "table",
            "type": "u32",
            "components": null
          }
        ]
      }
    ],
    "name": "struct_inside_enum",
    "outputs": []
  }
]
        "#;

        let contract = Abigen::new("custom", contract).unwrap();
        assert_eq!(contract.custom_structs.len(), 1);
        assert_eq!(contract.custom_enums.len(), 1);
    }
}
#[test]
fn test_abigen_enum_inside_struct() {
    let contract = r#"
[
  {
    "type": "function",
    "inputs": [
      {
        "name": "c",
        "type": "struct Cocktail",
        "components": [
          {
            "name": "shaker",
            "type": "enum Shaker",
            "components": [
              {
                "name": "Cosmopolitan",
                "type": "bool",
                "components": null
              },
              {
                "name": "Mojito",
                "type": "u32",
                "components": null
              }
            ]
          },
          {
            "name": "glass",
            "type": "u64",
            "components": null
          }
        ]
      }
    ],
    "name": "give_and_return_enum_inside_struct",
    "outputs": []
  }
]
        "#;

    let contract = Abigen::new("custom", contract).unwrap();
    assert_eq!(contract.custom_structs.len(), 1);
    assert_eq!(contract.custom_enums.len(), 1);
}
