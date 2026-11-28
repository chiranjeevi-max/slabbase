use std::collections::BTreeSet;
use std::fmt;
use std::io::Read;

use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};

use super::{ImportAdapter, ImportError, ImportFormat, ImportOptions, SourceRow, SourceValue};

/// CSV adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct CsvAdapter;

impl ImportAdapter for CsvAdapter {
    fn format(&self) -> ImportFormat {
        ImportFormat::Csv
    }

    fn read(
        &mut self,
        reader: &mut dyn Read,
        options: &ImportOptions,
        on_columns: &mut dyn FnMut(&[String]),
        on_row: &mut dyn FnMut(SourceRow),
    ) -> Result<(), ImportError> {
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(options.delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(reader);
        let mut records = csv_reader.records();
        let first = match records.next() {
            Some(record) => Some(record?),
            None => return Ok(()),
        };
        let headers: Vec<String> = if options.has_headers {
            first
                .as_ref()
                .map(|record| {
                    record
                        .iter()
                        .enumerate()
                        .map(|(index, value)| {
                            let value = value.strip_prefix('\u{FEFF}').unwrap_or(value).trim();
                            if value.is_empty() {
                                format!("Column {}", index + 1)
                            } else {
                                value.to_owned()
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            (0..first.as_ref().map(csv::StringRecord::len).unwrap_or(0))
                .map(|index| format!("Column {}", index + 1))
                .collect()
        };
        let mut seen = BTreeSet::new();
        for header in &headers {
            if !seen.insert(header.clone()) {
                return Err(ImportError::Format(format!("duplicate header {header:?}")));
            }
        }
        on_columns(&headers);
        let mut row_number = if options.has_headers { 0 } else { 1 };
        if !options.has_headers
            && let Some(record) = first
        {
            emit_csv_row(&headers, &record, 1, true, on_row);
        }
        for record in records {
            row_number += 1;
            let record = record?;
            emit_csv_row(&headers, &record, row_number, false, on_row);
        }
        Ok(())
    }
}

fn emit_csv_row(
    headers: &[String],
    record: &csv::StringRecord,
    row_number: u64,
    strip_bom: bool,
    on_row: &mut dyn FnMut(SourceRow),
) {
    let mut row = SourceRow {
        number: row_number,
        empty_text_is_null: true,
        ..SourceRow::default()
    };
    if record.len() != headers.len() {
        row.issues.push(format!(
            "row has {} fields but the header has {}",
            record.len(),
            headers.len()
        ));
    }
    for (index, header) in headers.iter().enumerate() {
        let value = record.get(index).map(|value| {
            if strip_bom && index == 0 {
                value.strip_prefix('\u{FEFF}').unwrap_or(value).to_owned()
            } else {
                value.to_owned()
            }
        });
        row.values.insert(
            header.clone(),
            value.map_or(SourceValue::Null, SourceValue::Text),
        );
    }
    on_row(row);
}

/// JSON adapter for arrays of objects and NDJSON.
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonAdapter;

impl ImportAdapter for JsonAdapter {
    fn format(&self) -> ImportFormat {
        ImportFormat::Json
    }

    fn read(
        &mut self,
        reader: &mut dyn Read,
        _options: &ImportOptions,
        on_columns: &mut dyn FnMut(&[String]),
        on_row: &mut dyn FnMut(SourceRow),
    ) -> Result<(), ImportError> {
        // Strip one UTF-8 BOM, as the CSV adapter does. A BOM is too short to
        // peek with a buffer and then un-read, so read the three-byte prefix
        // and chain it back unless it is a BOM.
        let mut prefix = [0_u8; 3];
        let mut filled = 0;
        while filled < prefix.len() {
            match reader.read(&mut prefix[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        let mut source = if prefix[..filled] == [0xEF, 0xBB, 0xBF] {
            std::io::Cursor::new(Vec::new()).chain(reader)
        } else {
            std::io::Cursor::new(prefix[..filled].to_vec()).chain(reader)
        };
        let mut tail = TailReader {
            inner: &mut source,
            last_non_whitespace: None,
        };
        let mut deserializer = serde_json::Deserializer::from_reader(&mut tail);
        let mut columns: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut row_number = 0_u64;
        let eof = loop {
            let visitor = JsonVisitor {
                columns: &mut columns,
                seen: &mut seen,
                on_columns,
                on_row,
                row_number: &mut row_number,
                array_row: false,
            };
            match visitor.deserialize(&mut deserializer) {
                Ok(()) => {}
                Err(error) if error.is_eof() => break Some(error),
                Err(error) => return Err(error.into()),
            }
        };
        drop(deserializer);
        if let Some(error) = eof {
            // serde_json reports a clean end-of-stream and a value truncated
            // at EOF with the same error category. A clean end after a
            // complete top-level value ends in `}` or `]` (or only whitespace
            // was present); anything else is a truncated document.
            let clean = tail.last_non_whitespace.is_none()
                || (error.to_string().contains("EOF while parsing a value")
                    && matches!(tail.last_non_whitespace, Some(b'}') | Some(b']')));
            if !clean {
                return Err(error.into());
            }
        }
        Ok(())
    }
}

struct TailReader<'a> {
    inner: &'a mut dyn Read,
    last_non_whitespace: Option<u8>,
}

impl Read for TailReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        for byte in &buffer[..read] {
            if !byte.is_ascii_whitespace() {
                self.last_non_whitespace = Some(*byte);
            }
        }
        Ok(read)
    }
}

struct JsonVisitor<'a> {
    columns: &'a mut Vec<String>,
    seen: &'a mut BTreeSet<String>,
    on_columns: &'a mut dyn FnMut(&[String]),
    on_row: &'a mut dyn FnMut(SourceRow),
    row_number: &'a mut u64,
    array_row: bool,
}

impl<'de> DeserializeSeed<'de> for JsonVisitor<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for JsonVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an object or an array of objects")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        if self.array_row {
            while sequence.next_element::<IgnoredAny>()?.is_some() {}
            return self.non_object();
        }
        while sequence
            .next_element_seed(JsonVisitor {
                columns: &mut *self.columns,
                seen: &mut *self.seen,
                on_columns: &mut *self.on_columns,
                on_row: &mut *self.on_row,
                row_number: &mut *self.row_number,
                array_row: true,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<(), A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            let value: serde_json::Value = map.next_value()?;
            fields.push((key, source_value(value)));
        }
        self.emit_object(fields);
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<(), E>
    where
        E: de::Error,
    {
        self.non_object()
    }

    fn visit_bool<E>(self, _value: bool) -> Result<(), E>
    where
        E: de::Error,
    {
        self.non_object()
    }

    fn visit_i64<E>(self, _value: i64) -> Result<(), E>
    where
        E: de::Error,
    {
        self.non_object()
    }

    fn visit_u64<E>(self, _value: u64) -> Result<(), E>
    where
        E: de::Error,
    {
        self.non_object()
    }

    fn visit_f64<E>(self, _value: f64) -> Result<(), E>
    where
        E: de::Error,
    {
        self.non_object()
    }

    fn visit_unit<E>(self) -> Result<(), E>
    where
        E: de::Error,
    {
        self.non_object()
    }
}

impl JsonVisitor<'_> {
    fn non_object<E: de::Error>(self) -> Result<(), E> {
        if !self.array_row {
            return Err(E::custom(
                "JSON source must be an object or an array of objects",
            ));
        }
        *self.row_number = self.row_number.saturating_add(1);
        (self.on_row)(SourceRow {
            number: *self.row_number,
            issues: vec!["row is not an object".into()],
            ..SourceRow::default()
        });
        Ok(())
    }

    fn emit_object(&mut self, fields: Vec<(String, SourceValue)>) {
        *self.row_number = self.row_number.saturating_add(1);
        let mut row = SourceRow {
            number: *self.row_number,
            ..SourceRow::default()
        };
        let previous_columns = self.columns.len();
        for (key, value) in fields {
            if self.seen.insert(key.clone()) {
                self.columns.push(key.clone());
            }
            row.values.insert(key, value);
        }
        if self.columns.len() != previous_columns {
            (self.on_columns)(self.columns);
        }
        for column in self.columns.iter() {
            if !row.values.contains_key(column) {
                row.values.insert(column.clone(), SourceValue::Null);
            }
        }
        (self.on_row)(row);
    }
}

/// A JSON cell keeps its shape until the target column type is known.
fn source_value(value: serde_json::Value) -> SourceValue {
    match value {
        serde_json::Value::Null => SourceValue::Null,
        serde_json::Value::String(text) => SourceValue::Text(text),
        other => SourceValue::Json(other),
    }
}
