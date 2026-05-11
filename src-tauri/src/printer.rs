use std::path::Path;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "outcome")]
pub enum PrintResult {
    Printed,
    OpenedForManualPrint,
    Failed { reason: String },
}

// ─── Windows ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub fn print_file(path: &str) -> PrintResult {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
    // SE_ERR_NOASSOC: the file type has no registered handler for the requested verb (code 31).
    const SE_ERR_NOASSOC: usize = 31;

    let file = Path::new(path);
    if !file.exists() {
        return PrintResult::Failed { reason: format!("File not found: {path}") };
    }
    if !file.is_file() {
        return PrintResult::Failed { reason: format!("Not a file: {path}") };
    }
    let abs_path = match file.canonicalize() {
        Ok(p) => p,
        Err(e) => return PrintResult::Failed { reason: format!("Could not resolve path: {e}") },
    };
    let abs_str    = abs_path.to_string_lossy().to_string();
    let clean_path = abs_str.trim_start_matches(r"\\?\");
    let path_h     = HSTRING::from(clean_path);

    let print_result = unsafe {
        ShellExecuteW(None, &HSTRING::from("print"), &path_h, None, None, SW_HIDE)
    };
    if print_result.0 as usize > 32 {
        log::info!("[print_file] sent to printer: {}", file.file_name().unwrap_or_default().to_string_lossy());
        return PrintResult::Printed;
    }
    let code = print_result.0 as usize;
    if code == SE_ERR_NOASSOC {
        log::warn!("[print_file] no print handler for '{}' (SE_ERR_NOASSOC=31); falling back to open verb", clean_path);
        let open_result = unsafe {
            ShellExecuteW(None, &HSTRING::from("open"), &path_h, None, None, SW_SHOWNORMAL)
        };
        if open_result.0 as usize > 32 {
            log::info!("[print_file] opened '{}' in default app for manual print", clean_path);
            return PrintResult::OpenedForManualPrint;
        }
        return PrintResult::Failed {
            reason: format!(
                "No print handler registered for this file type, and the open verb \
                 also failed (code {}). No default app may be associated.",
                open_result.0 as usize
            ),
        };
    }
    PrintResult::Failed {
        reason: format!("Failed to print '{}'. ShellExecuteW code: {}.", clean_path, code),
    }
}

#[cfg(target_os = "windows")]
pub fn convert_to_pdf(input_path: &str, output_path: &str) -> Result<String, String> {
    let input = Path::new(input_path);
    if !input.exists() {
        return Err(format!("Input file not found: {input_path}"));
    }
    let abs_in = input.canonicalize()
        .map_err(|e| format!("Could not resolve input path: {e}"))?
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    let output = Path::new(output_path);
    let abs_out = if output.is_absolute() {
        output.to_string_lossy().to_string()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(output)
            .to_string_lossy()
            .to_string()
    };
    let ext = input.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .ok_or("Input file has no extension")?;
    let ps_script = match ext.as_str() {
        "docx" | "doc" => format!(
            r#"$word = New-Object -ComObject Word.Application; $word.Visible = $false; $doc = $word.Documents.Open('{}'); $doc.SaveAs([ref]'{}', [ref]17); $doc.Close(); $word.Quit()"#,
            abs_in.replace('\'', "''"), abs_out.replace('\'', "''")
        ),
        "xlsx" | "xls" => format!(
            r#"$excel = New-Object -ComObject Excel.Application; $excel.Visible = $false; $wb = $excel.Workbooks.Open('{}'); $wb.ExportAsFixedFormat(0, '{}'); $wb.Close($false); $excel.Quit()"#,
            abs_in.replace('\'', "''"), abs_out.replace('\'', "''")
        ),
        "pptx" | "ppt" => format!(
            r#"$ppt = New-Object -ComObject PowerPoint.Application; $pres = $ppt.Presentations.Open('{}', $true, $false, $false); $pres.SaveAs('{}', 32); $pres.Close(); $ppt.Quit()"#,
            abs_in.replace('\'', "''"), abs_out.replace('\'', "''")
        ),
        _ => return Err(format!(
            "Unsupported file type for PDF conversion: .{ext}. Supported: docx, xlsx, pptx."
        )),
    };
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &ps_script]);
    crate::process_utils::no_window(&mut cmd);
    let result = cmd.output()
        .map_err(|e| format!("Failed to run PowerShell: {e}"))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!(
            "Conversion failed. Make sure Microsoft Office is installed. Error: {}",
            stderr.trim()
        ));
    }
    if !Path::new(&abs_out).exists() {
        return Err(
            "Conversion command completed but output file was not created. \
             Check that Office is installed and the input file isn't corrupted."
                .to_string(),
        );
    }
    Ok(format!("Converted to PDF: {abs_out}"))
}

// ─── macOS ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
pub fn print_file(path: &str) -> PrintResult {
    let file = Path::new(path);
    if !file.exists() {
        return PrintResult::Failed { reason: format!("File not found: {path}") };
    }
    if !file.is_file() {
        return PrintResult::Failed { reason: format!("Not a file: {path}") };
    }
    // Attempt 1: lp (CUPS print queue)
    match std::process::Command::new("lp").arg(path).status() {
        Ok(s) if s.success() => {
            log::info!("[print_file] sent to printer via lp: {path}");
            return PrintResult::Printed;
        }
        _ => log::warn!("[print_file] lp failed for '{}'; falling back to open", path),
    }
    // Attempt 2: open in default app for manual print
    match std::process::Command::new("open").arg(path).spawn() {
        Ok(_) => {
            log::info!("[print_file] opened '{}' in default app for manual print", path);
            PrintResult::OpenedForManualPrint
        }
        Err(e) => PrintResult::Failed {
            reason: format!("lp failed and open also failed: {e}"),
        },
    }
}

#[cfg(target_os = "macos")]
pub fn convert_to_pdf(_input_path: &str, _output_path: &str) -> Result<String, String> {
    // TODO(mac): PDF conversion requires a different approach on macOS (no Office COM automation).
    // Options: LibreOffice CLI, pandoc, or a native Swift/AppleScript solution.
    Err("convert_to_pdf is not available on macOS — no Office COM automation. \
         Use LibreOffice or an online converter."
        .to_string())
}
