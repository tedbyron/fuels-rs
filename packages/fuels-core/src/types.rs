use crate::errors::Error;
use anyhow::Result;
use proc_macro2::TokenStream;
use quote::quote;

use crate::ParamType;
use crate::Token;

/// Expands a [`ParamType`] into a TokenStream.
/// Used to expand functions when generating type-safe bindings of a JSON ABI.
pub fn expand_type(kind: &ParamType) -> Result<TokenStream, Error> {
    match kind {
        ParamType::U8 | ParamType::Byte => Ok(quote! { u8 }),
        ParamType::U16 => Ok(quote! { u16 }),
        ParamType::U32 => Ok(quote! { u32 }),
        ParamType::U64 => Ok(quote! { u64 }),
        ParamType::Bool => Ok(quote! { bool }),
        ParamType::B256 => Ok(quote! { [u8; 32] }),
        ParamType::String(_) => Ok(quote! { String }),
        ParamType::Array(t, _size) => {
            let inner = expand_type(t)?;
            Ok(quote! { ::std::vec::Vec<#inner> })
        }
        ParamType::Struct(members) => {
            if members.is_empty() {
                return Err(Error::InvalidData);
            }
            let members = members
                .iter()
                .map(expand_type)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(quote! { (#(#members,)*) })
        }
        ParamType::Enum(members) => {
            if members.is_empty() {
                return Err(Error::InvalidData);
            }
            let members = members
                .iter()
                .map(expand_type)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(quote! { (#(#members,)*) })
        }
        ParamType::Tuple(members) => {
            if members.is_empty() {
                return Err(Error::InvalidData);
            }
            let members = members
                .iter()
                .map(expand_type)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(quote! { (#(#members,)*) })
        }
    }
}

impl From<Token> for ParamType {
    fn from(t: Token) -> ParamType {
        match t {
            Token::U8(_) => ParamType::U8,
            Token::U16(_) => ParamType::U16,
            Token::U32(_) => ParamType::U32,
            Token::U64(_) => ParamType::U64,
            Token::Bool(_) => ParamType::Bool,
            Token::Byte(_) => ParamType::U8,
            Token::B256(_) => ParamType::B256,
            Token::Array(members) => {
                ParamType::Array(Box::new(ParamType::from(members[0].clone())), members.len())
            }
            Token::String(content) => ParamType::String(content.len()),
            Token::Struct(members) => ParamType::Struct(
                members
                    .iter()
                    .map(|token| ParamType::from(token.clone()))
                    .collect(),
            ),
            Token::Tuple(members) => ParamType::Tuple(
                members
                    .iter()
                    .map(|token| ParamType::from(token.clone()))
                    .collect(),
            ),
            // TODO(vnepveu): figure out how to convert the Token::Enum properly (w/ the Box type…)
            _ => ParamType::Enum(vec![]),
        }
    }
}
