//! Student import service — parses CSV, TSV, and Excel files for bulk enrollment.
//!
//! This service handles:
//! - Auto-detection of file format (CSV, TSV, .xlsx)
//! - Row-level validation (student_id + display_name required)
//! - Duplicate detection within uploaded file
//! - Integration with existing bulk-enroll insert-or-skip logic

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Parsed row from import file
#[derive(Debug, Clone, Deserialize)]
pub struct RawRow {
    pub student_id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub notes: Option<String>,
}

/// Validated row ready for import
#[derive(Debug, Clone)]
pub struct ValidatedRow {
    pub row_number: usize,
    pub student_id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub notes: Option<String>,
}

/// Row-level error during import
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowError {
    pub row: usize,
    pub reason: String,
}

/// Import result returned to frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub enrolled: usize,
    pub skipped: usize,
    pub errors: Vec<RowError>,
    pub students: Vec<serde_json::Value>,
}

/// Detect file format from extension
pub fn detect_format(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".xlsx") || lower.ends_with(".xlsm") || lower.ends_with(".xlsb") {
        "xlsx"
    } else if lower.ends_with(".tsv") {
        "tsv"
    } else {
        "csv"
    }
}

/// Parse CSV/TSV content into raw rows
pub fn parse_text_file(content: &[u8], delimiter: char) -> Result<Vec<RawRow>, String> {
    use std::str;
    
    // Handle BOM
    let text = if content.starts_with(&[0xEF, 0xBB, 0xBF]) {
        str::from_utf8(&content[3..]).map_err(|e| e.to_string())?
    } else {
        str::from_utf8(content).map_err(|e| e.to_string())?
    };
    
    let mut lines = text.lines();
    
    // Read header
    let header_line = lines.next().ok_or("File is empty")?.trim();
    let headers: Vec<String> = header_line.split(delimiter)
        .map(|h| h.trim().to_lowercase())
        .collect();
    
    // Find required columns
    let student_id_idx = headers.iter().position(|h| h == "student_id")
        .ok_or("Missing required column: student_id")?;
    let display_name_idx = headers.iter().position(|h| h == "display_name")
        .ok_or("Missing required column: display_name")?;
    
    let email_idx = headers.iter().position(|h| h == "email");
    let notes_idx = headers.iter().position(|h| h == "notes");
    
    let mut rows = Vec::new();
    
    for (line_num, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        
        let values: Vec<&str> = line.split(delimiter).collect();
        
        let student_id = values.get(student_id_idx).unwrap_or(&"").trim().to_string();
        let display_name = values.get(display_name_idx).unwrap_or(&"").trim().to_string();
        let email = email_idx.and_then(|i| values.get(i)).map(|v| v.trim().to_string());
        let notes = notes_idx.and_then(|i| values.get(i)).map(|v| v.trim().to_string());
        
        if student_id.is_empty() || display_name.is_empty() {
            continue; // Skip rows with missing required fields
        }
        
        rows.push(RawRow {
            student_id,
            display_name,
            email,
            notes,
        });
    }
    
    Ok(rows)
}

/// Parse Excel file using calamine
#[cfg(feature = "excel")]
pub fn parse_xlsx(content: &[u8]) -> Result<Vec<RawRow>, String> {
    use calamine::{Reader, Xlsx, Data};
    
    let mut workbook: Xlsx<_> = Xlsx::reader()
        .open_from_memory(content)
        .map_err(|e| e.to_string())?;
    
    let sheets = workbook.sheet_names().to_vec();
    if sheets.is_empty() {
        return Err("No sheets found in workbook".to_string());
    }
    
    let sheet_name = &sheets[0];
    let range = workbook.sheet_range(sheet_name)
        .map_err(|e| e.to_string())?;
    
    let mut rows = Vec::new();
    
    // Get header row
    let header = range.rows().next();
    if let Some(header) = header {
        let mut headers: Vec<String> = header.iter().map(|cell| {
            cell.to_string().to_lowercase()
        }).collect();
        
        let student_id_idx = headers.iter().position(|h| h == "student_id");
        let display_name_idx = headers.iter().position(|h| h == "display_name");
        
        if student_id_idx.is_none() || display_name_idx.is_none() {
            return Err(format!(
                "Missing required columns. Found: {:?}",
                headers
            ));
        }
        
        let student_id_idx = student_id_idx.unwrap();
        let display_name_idx = display_name_idx.unwrap();
        let email_idx = headers.iter().position(|h| h == "email");
        let notes_idx = headers.iter().position(|h| h == "notes");
        
        for (line_num, row) in range.rows().skip(1).enumerate() {
            let student_id = row.get(student_id_idx).map(|c| c.to_string()).unwrap_or_default();
            let display_name = row.get(display_name_idx).map(|c| c.to_string()).unwrap_or_default();
            
            if student_id.is_empty() || display_name.is_empty() {
                continue;
            }
            
            let email = email_idx.and_then(|i| row.get(i)).map(|c| c.to_string());
            let notes = notes_idx.and_then(|i| row.get(i)).map(|c| c.to_string());
            
            rows.push(RawRow {
                student_id,
                display_name,
                email,
                notes,
            });
        }
    }
    
    Ok(rows)
}

/// Validate rows and detect duplicates within the file
pub fn validate_rows(rows: Vec<RawRow>) -> (Vec<ValidatedRow>, Vec<RowError>) {
    let mut validated = Vec::new();
    let mut errors = Vec::new();
    let mut seen_ids: HashMap<String, usize> = HashMap::new();
    
    for (idx, row) in rows.into_iter().enumerate() {
        let row_num = idx + 2; // +2 because row 1 is header, enumerate starts at 0
        
        if row.student_id.is_empty() {
            errors.push(RowError {
                row: row_num,
                reason: "Missing student_id".to_string(),
            });
            continue;
        }
        
        if row.display_name.is_empty() {
            errors.push(RowError {
                row: row_num,
                reason: "Missing display_name".to_string(),
            });
            continue;
        }
        
        if let Some(first_seen) = seen_ids.get(&row.student_id) {
            errors.push(RowError {
                row: row_num,
                reason: format!("Duplicate student_id: {}", row.student_id),
            });
            continue;
        }
        
        seen_ids.insert(row.student_id.clone(), row_num);
        
        validated.push(ValidatedRow {
            row_number: row_num,
            student_id: row.student_id,
            display_name: row.display_name,
            email: row.email,
            notes: row.notes,
        });
    }
    
    (validated, errors)
}

/// Generate a CSV template with headers only
pub fn generate_template(delimiter: char) -> String {
    format!("student_id{del}display_name{del}email{del}notes", del = delimiter)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_detect_format_csv() {
        assert_eq!(detect_format("students.csv"), "csv");
    }
    
    #[test]
    fn test_detect_format_tsv() {
        assert_eq!(detect_format("students.tsv"), "tsv");
    }
    
    #[test]
    fn test_detect_format_xlsx() {
        assert_eq!(detect_format("students.xlsx"), "xlsx");
    }
    
    #[test]
    fn test_parse_csv_basic() {
        let content = b"student_id,display_name,email,notes\njohn-doe,John Doe,john@example.com,\njane-smith,Jane Smith,jane@example.com,";
        let rows = parse_text_file(content, ',').unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].student_id, "john-doe");
        assert_eq!(rows[1].display_name, "Jane Smith");
    }
    
    #[test]
    fn test_parse_tsv_auto_detect() {
        let content = b"student_id\tdisplay_name\njohn-doe\tJohn Doe";
        let rows = parse_text_file(content, '\t').unwrap();
        assert_eq!(rows.len(), 1);
    }
    
    #[test]
    fn test_validate_detect_duplicates() {
        let rows = vec![
            RawRow { student_id: "john".to_string(), display_name: "John".to_string(), email: None, notes: None },
            RawRow { student_id: "john".to_string(), display_name: "John Again".to_string(), email: None, notes: None },
        ];
        let (valid, errors) = validate_rows(rows);
        assert_eq!(valid.len(), 1);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].reason.contains("Duplicate"));
    }
    
    #[test]
    fn test_generate_template() {
        let template = generate_template(',');
        assert!(template.contains("student_id"));
        assert!(template.contains("display_name"));
    }
}
