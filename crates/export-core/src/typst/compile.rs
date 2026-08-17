use std::collections::HashMap;

use super::world::TypstWorld;

pub fn compile_to_pdf(
    content: &str,
    files: HashMap<String, Vec<u8>>,
) -> Result<Vec<u8>, crate::Error> {
    let world = TypstWorld::new(content.to_string(), files);

    let document = typst::compile(&world)
        .output
        .map_err(|errors| crate::Error::TypstCompile(format!("{:?}", errors)))?;

    let options = typst_pdf::PdfOptions::default();
    let pdf = typst_pdf::pdf(&document, &options)
        .map_err(|errors| crate::Error::TypstPdf(format!("{:?}", errors)))?;

    Ok(pdf)
}
