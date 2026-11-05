# Place each upscaled sign centred on an A3 landscape page and write one PDF
from PIL import Image
import img2pdf, glob, os, sys
out=sys.argv[1]
MM=25.4; W,H=420,297; M=10  # page and margin in mm
pages=[]
for i in range(1,7):
    im=Image.open(f'crops/bien-{i}.png').convert('RGB')
    w,h=im.size
    s=min((W-2*M)/w,(H-2*M)/h)  # mm per px
    dpi=round(MM/s)
    page_px=(round(W/MM*dpi), round(H/MM*dpi))
    pg=Image.new('RGB',page_px,(255,255,255))
    pg.paste(im,((page_px[0]-w)//2,(page_px[1]-h)//2))
    fn=f'crops/page-{i}.jpg'; pg.save(fn,quality=95,dpi=(dpi,dpi)); pages.append(fn)
    print(i,'dpi',dpi, 'sign mm', round(w*s), round(h*s))
lay=img2pdf.get_layout_fun((img2pdf.mm_to_pt(W),img2pdf.mm_to_pt(H)))
open(out,'wb').write(img2pdf.convert(pages,layout_fun=lay))
