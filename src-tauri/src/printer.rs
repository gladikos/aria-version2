use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub fn print_file(path: &str) -> Result<String, String> {
    let file = Path::new(path);
    if !file.exists() {
        return Err(format!("File not found: {path}"));
    }
    if !file.is_file() {
        return Err(format!("Not a file: {path}"));
    }

    let abs_path = file.canonicalize()
        .map_err(|e| format!("Could not resolve path: {e}"))?;
    let abs_path_str = abs_path.to_string_lossy().to_string();
    let abs_path_clean = abs_path_str.trim_start_matches(r"\\?\");

    let verb    = HSTRING::from("print");
    let path_h  = HSTRING::from(abs_path_clean);

    let result = unsafe {
        ShellExecuteW(None, &verb, &path_h, None, None, SW_HIDE)
    };

    // ShellExecuteW returns >32 on success; 0-32 are SE_ERR_* codes.
    if result.0 as usize > 32 {
        let fname = file.file_name().unwrap_or_default().to_string_lossy();
        Ok(format!("Sent to printer: {fname}"))
    } else {
        Err(format!(
            "Failed to print '{}'. Code: {}. \
             The file type may not have a registered print handler, \
             or no default printer is set.",
            abs_path_clean, result.0 as usize
        ))
    }
}

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

    let result = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
        .output()
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
