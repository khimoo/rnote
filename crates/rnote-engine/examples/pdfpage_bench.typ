// 300 pages that each show the same scan. Typst embeds the image once, so the Pdf stays small,
// but every page still decodes it when rendered, like a scanned book.
#set page(paper: "a4", margin: 0pt)
#for i in range(300) {
  image("../tests/fixtures/pdf/scan-page.jpg", width: 100%, height: 100%, fit: "stretch")
  if i < 299 { pagebreak() }
}
