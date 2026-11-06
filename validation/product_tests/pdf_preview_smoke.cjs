/* Render a synthetic PDF using the same legacy module/worker as FilePreview.
 * Uses a disposable browser against Vite; requires PLAYWRIGHT_MODULE when not installed locally.
 */
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');

(async () => {
  const browser = await chromium.launch({ headless: true,
    ...(process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE } : {}) });
  try {
    const page = await browser.newPage();
    await page.route('**/__pdf_preview_test__', route => route.fulfill({ contentType: 'text/html', body: '<html><body></body></html>' }));
    await page.goto(`${process.env.SETUP_UI_BASE || 'http://127.0.0.1:1420'}/__pdf_preview_test__`);
    const result = await page.evaluate(async () => {
      const { getDocument, GlobalWorkerOptions } = await import('/node_modules/pdfjs-dist/legacy/build/pdf.mjs');
      GlobalWorkerOptions.workerSrc = (await import('/node_modules/pdfjs-dist/legacy/build/pdf.worker.min.mjs?url')).default;
      const text = 'BT /F1 18 Tf 30 80 Td (Athenaeum PDF check) Tj ET';
      const objects = [
        '<< /Type /Catalog /Pages 2 0 R >>',
        '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 140] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
        '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
        `<< /Length ${text.length} >>\nstream\n${text}\nendstream`,
      ];
      let pdf = '%PDF-1.4\n'; const offsets = [0];
      for (const [index, object] of objects.entries()) { offsets.push(pdf.length); pdf += `${index + 1} 0 obj\n${object}\nendobj\n`; }
      const xref = pdf.length;
      pdf += `xref\n0 6\n0000000000 65535 f \n${offsets.slice(1).map(offset => String(offset).padStart(10, '0') + ' 00000 n \n').join('')}trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
      const task = getDocument({ data: new TextEncoder().encode(pdf) });
      const document = await task.promise;
      try {
        const pdfPage = await document.getPage(1), viewport = pdfPage.getViewport({ scale: 1 });
        const canvas = globalThis.document.createElement('canvas'); canvas.width = viewport.width; canvas.height = viewport.height;
        const context = canvas.getContext('2d');
        await pdfPage.render({ canvas, canvasContext: context, viewport }).promise;
        const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
        let ink = 0; for (let index = 0; index < pixels.length; index += 4) if (pixels[index] < 200) ink++;
        return { pages: document.numPages, text: (await pdfPage.getTextContent()).items.map(item => item.str).join(' '), ink };
      } finally { await task.destroy(); }
    });
    assert.equal(result.pages, 1); assert.match(result.text, /Athenaeum PDF check/); assert.ok(result.ink > 100);
    console.log('PDF preview smoke passed: legacy worker loads, extracts text and renders visible content.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
