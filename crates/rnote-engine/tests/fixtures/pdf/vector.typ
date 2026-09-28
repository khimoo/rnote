#set page(paper: "a4", margin: 2cm)
#set text(size: 32pt)
#for i in range(1, 4) [
  = Page #i
  #lorem(40)
  #if i < 3 { pagebreak() }
]
