//
// Copyright (c) 2026 Marcus Schäfer
//
// This file is part of flake-pilot
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
use serde::Serialize;
use crate::defaults;

pub fn print_table(headline: &[&str], rows: &[Vec<String>]) {
    /*!
    Print rows as human readable table below the given headline
    !*/
    let headline: Vec<String> = headline
        .iter().map(|column| column.to_string()).collect();
    let mut widths: Vec<usize> = headline
        .iter().map(|column| column.chars().count()).collect();
    for row in rows {
        for (column, value) in row.iter().enumerate() {
            widths[column] = widths[column].max(value.chars().count());
        }
    }
    let ruler: Vec<String> = widths
        .iter().map(|width| "-".repeat(*width)).collect();
    print_table_row(&headline, &widths);
    print_table_row(&ruler, &widths);
    for row in rows {
        print_table_row(row, &widths);
    }
}

fn print_table_row(row: &[String], widths: &[usize]) {
    /*!
    Print one table row, columns padded to the given widths.
    The last column is not padded to avoid trailing blanks
    !*/
    let mut line = String::new();
    for (column, value) in row.iter().enumerate() {
        if column + 1 == row.len() {
            line.push_str(value);
        } else {
            let padding = widths[column] - value.chars().count();
            line.push_str(value);
            line.push_str(&" ".repeat(padding));
            line.push_str(defaults::FLAKE_LIST_COLUMN_SPACING);
        }
    }
    println!("{line}");
}

pub fn print_json<T: Serialize>(records: &T) {
    /*!
    Print records as JSON, machine readable. Values which
    could not be read are set to null
    !*/
    match serde_json::to_string_pretty(records) {
        Ok(json) => println!("{json}"),
        Err(error) => panic!("Failed to serialize records: {:?}", error)
    }
}

pub fn print_csv(rows: &[Vec<String>]) {
    /*!
    Print rows as comma separated values, machine readable.
    Values which could not be read are printed as empty fields
    !*/
    for row in rows {
        let fields: Vec<String> = row
            .iter().map(|field| csv_field(field)).collect();
        println!("{}", fields.join(","));
    }
}

fn csv_field(value: &str) -> String {
    /*!
    Quote a CSV field if it contains characters with a
    special meaning in CSV data
    !*/
    if value.chars().any(|char| matches!(char, ',' | '"' | '\n' | '\r')) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn column_value(value: Option<&String>) -> String {
    /*!
    Table representation of an optional value
    !*/
    match value {
        Some(value) => value.to_string(),
        None => defaults::FLAKE_LIST_NO_VALUE.to_string()
    }
}
