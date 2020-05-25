// LNP/BP Core Library implementing LNPBP specifications & standards
// Written in 2020 by
//     Dr. Maxim Orlovsky <orlovsky@pandoracore.com>
//
// To the extent possible under law, the author(s) have dedicated all
// copyright and related and neighboring rights to this software to
// the public domain worldwide. This software is distributed without
// any warranty.
//
// You should have received a copy of the MIT License
// along with this software.
// If not, see <https://opensource.org/licenses/MIT>.

use core::any::Any;
use core::convert::TryInto;
use core::marker::PhantomData;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

use super::tlv;
use super::{Encode, Error, EvenOdd, UnknownTypeError, Unmarshall, UnmarshallFn};
use crate::common::{AsAny, Wrapper};
use crate::lnp::presentation::tlv::Stream;
use crate::strict_encoding::{StrictDecode, StrictEncode};

wrapper!(
    Type,
    u16,
    doc = "Message type field value",
    derive = [Copy, PartialEq, Eq, PartialOrd, Ord, Hash]
);

impl EvenOdd for Type {}

#[derive(Clone, Debug, Display)]
#[display_from(Debug)]
pub struct Payload(Vec<Arc<dyn Any>>);

pub trait Message: AsAny {
    fn get_type(&self) -> Type;

    fn to_type<T>(&self) -> T
    where
        Self: Sized,
        Type: Into<T>,
    {
        self.get_type().into()
    }

    fn try_to_type<T>(&self) -> Result<T, <Type as TryInto<T>>::Error>
    where
        Self: Sized,
        Type: TryInto<T>,
    {
        self.get_type().try_into()
    }

    fn get_payload(&self) -> Payload;

    fn get_tlvs(&self) -> tlv::Stream;
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Display)]
#[display_from(Debug)]
pub struct RawMessage {
    pub type_id: Type,
    pub payload: Vec<u8>,
}

impl AsAny for RawMessage {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Message for RawMessage {
    fn get_type(&self) -> Type {
        self.type_id
    }

    fn get_payload(&self) -> Payload {
        Payload(vec![Arc::new(self.payload.clone())])
    }

    fn get_tlvs(&self) -> Stream {
        Stream::new()
    }
}

impl Encode for RawMessage {
    type Error = Error;

    fn encode(&self, mut e: &mut impl io::Write) -> Result<usize, Self::Error> {
        let mut len = 0usize;
        self.type_id
            .to_inner()
            .strict_encode(&mut e)
            .map_err(|_| Error::Io)?;
        len += e.write(&self.payload)?;
        Ok(len)
    }
}

pub trait EncodeRaw
where
    RawMessage: From<Self>,
    Self: Sized + Clone,
{
}

impl<T> Encode for T
where
    T: EncodeRaw,
    RawMessage: From<T>,
{
    type Error = Error;

    fn encode(&self, e: &mut impl io::Write) -> Result<usize, Self::Error> {
        RawMessage::from(self.clone()).encode(e)
    }
}

pub trait TypedEnum
where
    Self: Sized,
{
    fn try_from_type(type_id: Type, data: &dyn Any) -> Result<Self, UnknownTypeError>;
    fn get_type(&self) -> Type;
}

pub struct Unmarshaller<T>
where
    T: TypedEnum,
{
    known_types: BTreeMap<Type, UnmarshallFn<Error>>,
    _phantom: PhantomData<T>,
}

impl<T> Unmarshall for Unmarshaller<T>
where
    T: TypedEnum,
{
    type Data = Arc<T>;
    type Error = Error;

    fn unmarshall(&self, mut reader: &mut impl io::Read) -> Result<Self::Data, Self::Error> {
        let type_id = Type(u16::strict_decode(&mut reader).map_err(|_| Error::NoData)?);
        match self.known_types.get(&type_id) {
            None if type_id.is_even() => Err(Error::MessageEvenType),
            None => {
                let mut payload = Vec::new();
                reader.read_to_end(&mut payload)?;
                Ok(Arc::new(T::try_from_type(
                    type_id,
                    &RawMessage { type_id, payload },
                )?))
            }
            Some(parser) => parser(&mut reader)
                .and_then(|data| Ok(Arc::new(T::try_from_type(type_id, &*data)?))),
        }
    }
}

impl<T> Unmarshaller<T>
where
    T: TypedEnum,
{
    pub fn new(known_types: BTreeMap<u16, UnmarshallFn<Error>>) -> Self {
        Self {
            known_types: known_types.into_iter().map(|(t, f)| (Type(t), f)).collect(),
            _phantom: PhantomData,
        }
    }
}