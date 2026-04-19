use crate::function::Parameter;
use crate::{Evaluator, NixValue, Result};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, Event};
use std::io::{Cursor, Write};

pub fn to_xml(value: &NixValue, evaluator: &Evaluator) -> Result<String> {
    let mut writer = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);

    // Write XML declaration with single quotes to match Nix format
    writer
        .get_mut()
        .write_all(b"<?xml version='1.0' encoding='utf-8'?>\n")
        .map_err(|e| crate::Error::UnsupportedExpression {
            reason: format!("XML error: {}", e),
        })?;

    // Write root element <expr>
    writer
        .write_event(Event::Start(BytesStart::new("expr")))
        .map_err(|e| crate::Error::UnsupportedExpression {
            reason: format!("XML error: {}", e),
        })?;

    serialize_value(value, evaluator, &mut writer)?;

    // Close root element </expr>
    writer
        .write_event(Event::End(BytesEnd::new("expr")))
        .map_err(|e| crate::Error::UnsupportedExpression {
            reason: format!("XML error: {}", e),
        })?;

    let mut result = String::from_utf8(writer.into_inner().into_inner()).unwrap();

    // Nix format has a space before self-closing tags: <int value="10" />
    // quick-xml produces <int value="10"/>
    result = result.replace("/>", " />");

    // Add trailing newline to match expectation
    result.push('\n');

    Ok(result)
}

fn serialize_value<W: std::io::Write>(
    value: &NixValue,
    evaluator: &Evaluator,
    writer: &mut Writer<W>,
) -> Result<()> {
    let value = value.clone().force(evaluator)?;

    match value {
        NixValue::Integer(i) => {
            let mut elem = BytesStart::new("int");
            elem.push_attribute(("value", i.to_string().as_str()));
            writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
        }
        NixValue::Float(f) => {
            let mut elem = BytesStart::new("float");
            elem.push_attribute(("value", f.to_string().as_str()));
            writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
        }
        NixValue::String(s) => {
            let mut elem = BytesStart::new("string");
            elem.push_attribute(("value", s.as_str()));
            writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
        }
        NixValue::Boolean(b) => {
            let mut elem = BytesStart::new("bool");
            elem.push_attribute(("value", b.to_string().as_str()));
            writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
        }
        NixValue::Null => {
            writer
                .write_event(Event::Empty(BytesStart::new("null")))
                .map_err(xml_err)?;
        }
        NixValue::Path(p) => {
            let mut elem = BytesStart::new("path");
            elem.push_attribute(("value", p.to_string_lossy().as_ref()));
            writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
        }
        NixValue::StorePath(p) => {
            let mut elem = BytesStart::new("path");
            elem.push_attribute(("value", p.as_str()));
            writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
        }
        NixValue::List(l) => {
            writer
                .write_event(Event::Start(BytesStart::new("list")))
                .map_err(xml_err)?;
            for item in l {
                serialize_value(&item, evaluator, writer)?;
            }
            writer
                .write_event(Event::End(BytesEnd::new("list")))
                .map_err(xml_err)?;
        }
        NixValue::AttributeSet(attrs) => {
            writer
                .write_event(Event::Start(BytesStart::new("attrs")))
                .map_err(xml_err)?;
            let mut keys: Vec<_> = attrs.keys().collect();
            keys.sort();
            for key in keys {
                let val = attrs.get(key).unwrap();
                let mut attr_elem = BytesStart::new("attr");
                attr_elem.push_attribute(("name", key.as_str()));
                writer
                    .write_event(Event::Start(attr_elem))
                    .map_err(xml_err)?;
                serialize_value(val, evaluator, writer)?;
                writer
                    .write_event(Event::End(BytesEnd::new("attr")))
                    .map_err(xml_err)?;
            }
            writer
                .write_event(Event::End(BytesEnd::new("attrs")))
                .map_err(xml_err)?;
        }
        NixValue::Function(func) => {
            writer
                .write_event(Event::Start(BytesStart::new("function")))
                .map_err(xml_err)?;
            match &func.parameter {
                Parameter::Simple(name) => {
                    let mut elem = BytesStart::new("varpat");
                    elem.push_attribute(("name", name.as_str()));
                    writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
                }
                Parameter::Pattern {
                    name,
                    entries,
                    ellipsis,
                } => {
                    let mut elem = BytesStart::new("attrspat");
                    if let Some(n) = name {
                        elem.push_attribute(("name", n.as_str()));
                    }
                    if *ellipsis {
                        elem.push_attribute(("ellipsis", "1"));
                    }
                    writer.write_event(Event::Start(elem)).map_err(xml_err)?;
                    let mut sorted_entries = entries.clone();
                    sorted_entries.sort_by(|a, b| a.0.cmp(&b.0));
                    for (entry_name, _) in sorted_entries {
                        let mut entry_elem = BytesStart::new("attr");
                        entry_elem.push_attribute(("name", entry_name.as_str()));
                        writer
                            .write_event(Event::Empty(entry_elem))
                            .map_err(xml_err)?;
                    }
                    writer
                        .write_event(Event::End(BytesEnd::new("attrspat")))
                        .map_err(xml_err)?;
                }
            }
            writer
                .write_event(Event::End(BytesEnd::new("function")))
                .map_err(xml_err)?;
        }
        NixValue::Builtin(_) => {
            writer
                .write_event(Event::Start(BytesStart::new("function")))
                .map_err(xml_err)?;
            let mut elem = BytesStart::new("varpat");
            elem.push_attribute(("name", "x"));
            writer.write_event(Event::Empty(elem)).map_err(xml_err)?;
            writer
                .write_event(Event::End(BytesEnd::new("function")))
                .map_err(xml_err)?;
        }
        NixValue::Derivation(_) => {
            writer
                .write_event(Event::Empty(BytesStart::new("unevaluated")))
                .map_err(xml_err)?;
        }
        _ => {
            writer
                .write_event(Event::Empty(BytesStart::new("unevaluated")))
                .map_err(xml_err)?;
        }
    }
    Ok(())
}

fn xml_err<E: std::fmt::Display>(e: E) -> crate::Error {
    crate::Error::UnsupportedExpression {
        reason: format!("XML error: {}", e),
    }
}
