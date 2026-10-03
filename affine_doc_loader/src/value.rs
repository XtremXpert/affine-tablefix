use serde_json::{Map as JsonMap, Value as JsonValue};
use y_octo::{AHashMap, Any, Array, Map, ReadAny, ReadArray, ReadMap, ReadText, ReadValue, Text, TextDeltaOp, Value};

pub(super) trait MapRead: Clone {
  type Value: ValueRead<Map = Self>;

  fn get_value(&self, key: &str) -> Option<Self::Value>;
  fn entries(&self) -> impl Iterator<Item = (String, Self::Value)> + '_;
  fn keys(&self) -> impl Iterator<Item = &str> + '_;

  fn is_empty(&self) -> bool {
    self.values().next().is_none()
  }

  fn values(&self) -> impl Iterator<Item = Self::Value> + '_ {
    self.entries().map(|(_, value)| value)
  }
}

pub(super) trait ArrayRead: Clone {
  type Value: ValueRead<Array = Self>;

  fn items(&self) -> impl Iterator<Item = Self::Value> + '_;
}

pub(super) trait TextRead: Clone {
  fn text(&self) -> String;
  fn delta(&self) -> Vec<TextDeltaOp>;
}

pub(super) trait AnyRead {
  fn as_str(&self) -> Option<&str>;
  fn as_bool(&self) -> Option<bool>;
  fn as_i64(&self) -> Option<i64>;
  fn as_u64(&self) -> Option<u64>;
  fn as_f64(&self) -> Option<f64>;
  fn truthy(&self) -> bool;
  fn string_array(&self) -> Option<Vec<String>>;
  fn to_any(&self) -> Any;
}

pub(super) trait ValueRead: Clone {
  type Map: MapRead<Value = Self>;
  type Array: ArrayRead<Value = Self>;
  type Text: TextRead;
  type Scalar<'a>: AnyRead
  where
    Self: 'a;

  fn as_map_view(&self) -> Option<Self::Map>;
  fn as_array_view(&self) -> Option<Self::Array>;
  fn as_text_view(&self) -> Option<Self::Text>;
  fn as_any_read(&self) -> Option<Self::Scalar<'_>>;

  fn as_any_owned(&self) -> Option<Any> {
    self.as_any_read().map(|value| value.to_any())
  }
}

impl MapRead for Map {
  type Value = Value;

  fn get_value(&self, key: &str) -> Option<Self::Value> {
    self.get(key)
  }

  fn entries(&self) -> impl Iterator<Item = (String, Self::Value)> + '_ {
    self.iter().map(|(key, value)| (key.to_string(), value))
  }

  fn keys(&self) -> impl Iterator<Item = &str> + '_ {
    self.iter().map(|(key, _)| key)
  }

  fn is_empty(&self) -> bool {
    Map::is_empty(self)
  }

  fn values(&self) -> impl Iterator<Item = Self::Value> + '_ {
    self.iter().map(|(_, value)| value)
  }
}

impl ArrayRead for Array {
  type Value = Value;

  fn items(&self) -> impl Iterator<Item = Self::Value> + '_ {
    self.iter()
  }
}

impl TextRead for Text {
  fn text(&self) -> String {
    self.to_string()
  }

  fn delta(&self) -> Vec<TextDeltaOp> {
    self.to_delta()
  }
}

impl ValueRead for Value {
  type Map = Map;
  type Array = Array;
  type Text = Text;
  type Scalar<'a> = &'a Any;

  fn as_map_view(&self) -> Option<Map> {
    self.to_map()
  }

  fn as_array_view(&self) -> Option<Array> {
    self.to_array()
  }

  fn as_text_view(&self) -> Option<Text> {
    self.to_text()
  }

  fn as_any_read(&self) -> Option<Self::Scalar<'_>> {
    match self {
      Value::Any(value) => Some(value),
      _ => None,
    }
  }
}

impl<'a> MapRead for ReadMap<'a> {
  type Value = ReadValue<'a>;

  fn get_value(&self, key: &str) -> Option<Self::Value> {
    self.get(key)
  }

  fn entries(&self) -> impl Iterator<Item = (String, Self::Value)> + '_ {
    self.iter().map(|(key, value)| (key.to_string(), value))
  }

  fn keys(&self) -> impl Iterator<Item = &str> + '_ {
    (*self).keys()
  }

  fn is_empty(&self) -> bool {
    (*self).is_empty()
  }

  fn values(&self) -> impl Iterator<Item = Self::Value> + '_ {
    self.iter().map(|(_, value)| value)
  }
}

impl<'a> ArrayRead for ReadArray<'a> {
  type Value = ReadValue<'a>;

  fn items(&self) -> impl Iterator<Item = Self::Value> + '_ {
    self.iter()
  }
}

impl TextRead for ReadText<'_> {
  fn text(&self) -> String {
    self.to_string()
  }

  fn delta(&self) -> Vec<TextDeltaOp> {
    self.to_delta()
  }
}

impl<'a> ValueRead for ReadValue<'a> {
  type Map = ReadMap<'a>;
  type Array = ReadArray<'a>;
  type Text = ReadText<'a>;
  type Scalar<'b>
    = ReadAny<'a>
  where
    Self: 'b;

  fn as_map_view(&self) -> Option<ReadMap<'a>> {
    self.as_map()
  }

  fn as_array_view(&self) -> Option<ReadArray<'a>> {
    self.as_array()
  }

  fn as_text_view(&self) -> Option<ReadText<'a>> {
    self.as_text()
  }

  fn as_any_read(&self) -> Option<Self::Scalar<'_>> {
    self.as_any()
  }
}

impl AnyRead for Any {
  fn as_str(&self) -> Option<&str> {
    match self {
      Any::String(value) => Some(value),
      _ => None,
    }
  }

  fn as_bool(&self) -> Option<bool> {
    match self {
      Any::True => Some(true),
      Any::False => Some(false),
      _ => None,
    }
  }

  fn as_i64(&self) -> Option<i64> {
    match self {
      Any::Integer(value) => Some(i64::from(*value)),
      Any::BigInt64(value) => Some(*value),
      Any::Float32(value) => Some(value.0 as i64),
      Any::Float64(value) => Some(value.0 as i64),
      _ => None,
    }
  }

  fn as_u64(&self) -> Option<u64> {
    match self {
      Any::Integer(value) if *value >= 0 => Some(*value as u64),
      Any::BigInt64(value) if *value >= 0 => Some(*value as u64),
      Any::Float32(value) if value.0 >= 0.0 => Some(value.0 as u64),
      Any::Float64(value) if value.0 >= 0.0 => Some(value.0 as u64),
      _ => None,
    }
  }

  fn as_f64(&self) -> Option<f64> {
    match self {
      Any::Integer(value) => Some(f64::from(*value)),
      Any::BigInt64(value) => Some(*value as f64),
      Any::Float32(value) => Some(f64::from(value.0)),
      Any::Float64(value) => Some(value.0),
      _ => None,
    }
  }

  fn truthy(&self) -> bool {
    match self {
      Any::True => true,
      Any::False | Any::Null | Any::Undefined => false,
      Any::String(value) => !value.is_empty(),
      Any::Integer(value) => *value != 0,
      Any::Float32(value) => value.0 != 0.0,
      Any::Float64(value) => value.0 != 0.0,
      Any::BigInt64(value) => *value != 0,
      Any::Object(_) | Any::Array(_) | Any::Binary(_) => true,
    }
  }

  fn string_array(&self) -> Option<Vec<String>> {
    match self {
      Any::Array(values) => Some(
        values
          .iter()
          .filter_map(|value| value.as_str().map(str::to_string))
          .collect(),
      ),
      _ => None,
    }
  }

  fn to_any(&self) -> Any {
    self.clone()
  }
}

impl<T: AnyRead + ?Sized> AnyRead for &T {
  fn as_str(&self) -> Option<&str> {
    T::as_str(self)
  }

  fn as_bool(&self) -> Option<bool> {
    T::as_bool(self)
  }

  fn as_i64(&self) -> Option<i64> {
    T::as_i64(self)
  }

  fn as_u64(&self) -> Option<u64> {
    T::as_u64(self)
  }

  fn as_f64(&self) -> Option<f64> {
    T::as_f64(self)
  }

  fn truthy(&self) -> bool {
    T::truthy(self)
  }

  fn string_array(&self) -> Option<Vec<String>> {
    T::string_array(self)
  }

  fn to_any(&self) -> Any {
    T::to_any(self)
  }
}

impl AnyRead for ReadAny<'_> {
  fn as_str(&self) -> Option<&str> {
    (*self).as_str()
  }

  fn as_bool(&self) -> Option<bool> {
    (*self).as_bool()
  }

  fn as_i64(&self) -> Option<i64> {
    (*self).as_i64().or_else(|| (*self).as_f64().map(|value| value as i64))
  }

  fn as_u64(&self) -> Option<u64> {
    if let Some(value) = (*self).as_i64() {
      return (value >= 0).then_some(value as u64);
    }
    (*self).as_f64().filter(|value| *value >= 0.0).map(|value| value as u64)
  }

  fn as_f64(&self) -> Option<f64> {
    (*self).as_f64()
  }

  fn truthy(&self) -> bool {
    if let Some(value) = (*self).as_bool() {
      return value;
    }
    if let Some(value) = (*self).as_str() {
      return !value.is_empty();
    }
    if let Some(value) = (*self).as_f64() {
      return value != 0.0;
    }
    !(*self).is_null() && !(*self).is_undefined()
  }

  fn string_array(&self) -> Option<Vec<String>> {
    (*self)
      .array()
      .map(|values| values.filter_map(|value| value.as_str().map(str::to_string)).collect())
  }

  fn to_any(&self) -> Any {
    (*self).to_owned()
  }
}

pub(super) fn any_truthy(value: &(impl AnyRead + ?Sized)) -> bool {
  value.truthy()
}

pub(super) fn any_as_string(value: &(impl AnyRead + ?Sized)) -> Option<&str> {
  value.as_str()
}

pub(super) fn any_as_u64(value: &(impl AnyRead + ?Sized)) -> Option<u64> {
  value.as_u64()
}

pub(super) fn value_to_string(value: &impl ValueRead) -> Option<String> {
  if let Some(text) = value.as_text_view() {
    return Some(text.text());
  }

  if let Some(any) = value.as_any_owned() {
    return any_to_string(&any);
  }

  None
}

pub(super) fn value_to_any(value: &impl ValueRead) -> Option<Any> {
  if let Some(any) = value.as_any_owned() {
    return Some(any);
  }

  if let Some(text) = value.as_text_view() {
    return Some(Any::String(text.text()));
  }

  if let Some(array) = value.as_array_view() {
    let mut values = Vec::new();
    for item in array.items() {
      if let Some(any) = value_to_any(&item) {
        values.push(any);
      } else if let Some(text) = value_to_string(&item) {
        values.push(Any::String(text));
      }
    }
    return Some(Any::Array(values));
  }

  if let Some(map) = value.as_map_view() {
    let mut values = AHashMap::default();
    for (key, entry) in map.entries() {
      if let Some(any) = value_to_any(&entry) {
        values.insert(key.clone(), any);
      } else if let Some(text) = value_to_string(&entry) {
        values.insert(key, Any::String(text));
      }
    }
    return Some(Any::Object(Box::new(values)));
  }

  None
}

pub(super) fn value_to_f64(value: impl ValueRead) -> Option<f64> {
  value.as_any_read().and_then(|any| any.as_f64())
}

pub(super) fn any_to_string(any: &Any) -> Option<String> {
  match any {
    Any::String(value) => Some(value.to_string()),
    Any::Integer(value) => Some(value.to_string()),
    Any::Float32(value) => Some(value.0.to_string()),
    Any::Float64(value) => Some(value.0.to_string()),
    Any::BigInt64(value) => Some(value.to_string()),
    Any::True => Some("true".into()),
    Any::False => Some("false".into()),
    Any::Null | Any::Undefined => None,
    Any::Array(_) | Any::Object(_) | Any::Binary(_) => serde_json::to_string(any).ok(),
  }
}

pub(super) fn params_any_map_to_json(params: &AHashMap<String, Any>) -> JsonValue {
  let mut values = JsonMap::new();
  for (key, value) in params.iter() {
    if let Ok(value) = serde_json::to_value(value) {
      values.insert(key.clone(), value);
    }
  }
  JsonValue::Object(values)
}

pub(super) fn params_value_to_json(params: &impl ValueRead) -> Option<JsonValue> {
  value_to_any(params).and_then(|value| serde_json::to_value(value).ok())
}

pub(super) fn build_reference_payload(doc_id: &str, params: Option<JsonValue>) -> String {
  let mut payload = JsonMap::new();
  payload.insert("docId".into(), JsonValue::String(doc_id.to_string()));
  if let Some(JsonValue::Object(params)) = params {
    for (key, value) in params.into_iter() {
      payload.insert(key, value);
    }
  }
  JsonValue::Object(payload).to_string()
}
