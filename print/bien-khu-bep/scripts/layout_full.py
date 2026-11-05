# Stretch every cut-out sign to the same size: the full A3 landscape page (420x297 mm, 300 dpi).
from PIL import Image
import img2pdf, sys
out=sys.argv[1]; W,H=420,297; DPI=300
px=(round(W/25.4*DPI), round(H/25.4*DPI))
pages=[]
for i in range(1,7):
    im=Image.open(f'crops/cut-{i}-alpha.png').convert('RGBA').resize(px, Image.LANCZOS)
    pg=Image.new('RGB',px,(255,255,255)); pg.paste(im,(0,0),im)
    fn=f'crops/full-{i}.jpg'; pg.save(fn,quality=94,dpi=(DPI,DPI)); pages.append(fn)
lay=img2pdf.get_layout_fun((img2pdf.mm_to_pt(W),img2pdf.mm_to_pt(H)))
open(out,'wb').write(img2pdf.convert(pages,layout_fun=lay))
