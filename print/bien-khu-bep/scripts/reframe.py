# Redraw a clean vector frame over each sign; keep the original artwork inside and decorations on top.
import cv2, numpy as np, json, math
PW,PH=420.0,297.0          # A3 landscape, mm
M=3.0                      # outer margin, mm
BW=10.0                    # how far the outer edge sits beyond the measured panel, mm
INSET=5.0                  # how far the new inner edge sits inside the measured panel, mm
T=BW+INSET                 # drawn band width
R_IN,RB_IN,F_IN=27.0,20.0,T+7.0   # inner corner radius, bump radius, concave fillet radius
fr=json.load(open('frames.json'))
RECT_MODE={1}
# decorations that overlap the old frame (x0,y0,x1,y1 in 4x crop px); lifted with GrabCut and put on top
STICKERS={1:[(140,1800,540,2360),(3000,1980,3360,2420)], 2:[(2560,1900,2900,2380)], 3:[(2740,1640,3200,2360)],
          4:[(150,1630,570,2330)], 5:[(2740,1780,3160,2110)], 6:[(2900,1740,3220,2240)]}

def arc(cx,cy,r,a0,a1,n=24):
    return [(cx+r*math.cos(a),cy+r*math.sin(a)) for a in np.linspace(a0,a1,n)]
def shape(L,R,Ts,B,bL,bR,Tb,r,rb,f):
    """Rounded rect with a raised rounded bump on top, concave fillets at the joins. Clockwise (y down)."""
    P=math.pi; pts=[]
    pts+=arc(L+r,Ts+r,r,P,1.5*P)                 # top-left corner
    pts+=arc(bL-f,Ts-f,f,0.5*P,0)[::1]           # concave fillet up into bump (centre above-left)
    pts+=arc(bL+rb,Tb+rb,rb,P,1.5*P)             # bump top-left
    pts+=arc(bR-rb,Tb+rb,rb,1.5*P,2*P)           # bump top-right
    pts+=arc(bR+f,Ts-f,f,P,0.5*P)                # concave fillet down
    pts+=arc(R-r,Ts+r,r,1.5*P,2*P)               # top-right
    pts+=arc(R-r,B-r,r,0,0.5*P)                  # bottom-right
    pts+=arc(L+r,B-r,r,0.5*P,P)                  # bottom-left
    return pts
def path(pts): return 'M '+' L '.join(f'{x:.2f} {y:.2f}' for x,y in pts)+' Z'

def cutout_alpha(img):
    hsv=cv2.cvtColor(img,cv2.COLOR_BGR2HSV)
    bg=((hsv[...,1]<45)&(hsv[...,2]>150)).astype(np.uint8)
    n,lab=cv2.connectedComponents(bg,connectivity=4)
    edge=set(np.unique(np.concatenate([lab[0],lab[-1],lab[:,0],lab[:,-1]])))-{0}
    return (~np.isin(lab,list(edge))).astype(np.uint8)

pages=[]
for i in range(1,7):
    f=fr[str(i)]
    img=cv2.imread(f'crops/bien-{i}.png'); h,w=img.shape[:2]
    # map crop px -> mm so the measured inner panel fills the new inner area
    x0,x1=M+BW, PW-M-BW; y0,y1=M+BW, PH-M-BW
    sx=(x1-x0)/(f['R']-f['L']); sy=(y1-y0)/(f['B']-f['Tb'])
    tx=x0-f['L']*sx; ty=y0-f['Tb']*sy
    X=lambda x:tx+x*sx; Y=lambda y:ty+y*sy
    inner=dict(L=x0+INSET,R=x1-INSET,Ts=Y(f['Ts'])+INSET,B=y1-INSET,bL=X(f['bL'])+INSET,bR=X(f['bR'])-INSET,Tb=y0+INSET)
    pin=shape(**inner,r=R_IN,rb=RB_IN,f=F_IN)
    pout=shape(inner['L']-T,inner['R']+T,inner['Ts']-T,inner['B']+T,inner['bL']-T,inner['bR']+T,inner['Tb']-T,R_IN+T,RB_IN+T,F_IN-T)
    # slightly smaller clip so the band always covers the old border edge
    # --- decorations that reach into the band: lift them onto a transparent overlay
    alpha=cutout_alpha(img)
    inpx=np.array([((x-tx)/sx,(y-ty)/sy) for x,y in pin],np.int32)
    inmask=np.zeros((h,w),np.uint8); cv2.fillPoly(inmask,[inpx],1)
    band=(alpha>0)&(inmask==0)
    lab=cv2.cvtColor(img,cv2.COLOR_BGR2LAB).astype(np.float32)
    bcol=np.median(lab[band],axis=0)
    hs=cv2.cvtColor(img,cv2.COLOR_BGR2HSV)[band].astype(float)
    sel=(hs[:,1]>=np.percentile(hs[:,1],55))&(hs[:,2]>=np.percentile(hs[:,2],35))&(hs[:,2]<=np.percentile(hs[:,2],90))
    pick=np.median(img[band][sel],axis=0)
    dist=np.linalg.norm(lab-bcol,axis=2)
    light=(lab[...,0]>235)
    near=cv2.dilate(band.astype(np.uint8),np.ones((61,61),np.uint8))>0
    cand=((dist>26)&~light&near&(alpha>0)).astype(np.uint8)
    cand=cv2.morphologyEx(cand,cv2.MORPH_OPEN,np.ones((5,5),np.uint8))
    n,cl,stt,_=cv2.connectedComponentsWithStats(cand,connectivity=8)
    rois=list(STICKERS.get(i,[]))
    bh=int(cv2.cvtColor(np.uint8([[pick]]),cv2.COLOR_BGR2HSV)[0,0,0])
    # merge overlapping boxes
    merged=[]
    for r in sorted(rois):
        r=[max(0,r[0]-30),max(0,r[1]-30),min(w,r[2]+30),min(h,r[3]+30)]
        for m_ in merged:
            if not (r[2]<m_[0] or r[0]>m_[2] or r[3]<m_[1] or r[1]>m_[3]):
                m_[:]=[min(m_[0],r[0]),min(m_[1],r[1]),max(m_[2],r[2]),max(m_[3],r[3])]; break
        else: merged.append(r)
    decotag=''; base=img.copy(); holes=np.zeros((h,w),np.uint8)
    print(i,'rois',merged)
    for j,(xa,ya,xb,yb_) in enumerate(merged):
        roi=img[ya:yb_,xa:xb]; sc=4
        small=cv2.resize(roi,None,fx=1/sc,fy=1/sc,interpolation=cv2.INTER_AREA)
        hs=cv2.cvtColor(small,cv2.COLOR_BGR2HSV).astype(int)
        hd_=np.abs(hs[...,0]-bh); hd_=np.minimum(hd_,180-hd_)
        creamy=(hs[...,2]>200)&(hs[...,1]<75)&(hs[...,0]>=8)&(hs[...,0]<=34)
        whiteish=(hs[...,1]<25)&(hs[...,2]>215)
        framey=(hd_<=6)&(hs[...,1]>35)
        gm=np.full(small.shape[:2],cv2.GC_PR_FGD,np.uint8)
        gm[creamy|framey|whiteish]=cv2.GC_PR_BGD
        gm[:2,:]=gm[-2:,:]=cv2.GC_BGD; gm[:,:2]=cv2.GC_BGD; gm[:,-2:]=cv2.GC_BGD
        b1=np.zeros((1,65)); f1=np.zeros((1,65))
        if i in RECT_MODE:   # frame and decoration share a hue: let GrabCut learn colours from a plain box
            gm=np.zeros(small.shape[:2],np.uint8)
            cv2.grabCut(small,gm,(3,3,small.shape[1]-6,small.shape[0]-6),b1,f1,8,cv2.GC_INIT_WITH_RECT)
        else:
            if not (gm==cv2.GC_PR_FGD).any(): continue
            cv2.grabCut(small,gm,None,b1,f1,8,cv2.GC_INIT_WITH_MASK)
        fm=((gm==1)|(gm==3)).astype(np.uint8)
        nn,ll,ss,_=cv2.connectedComponentsWithStats(fm)
        if nn<2: continue
        fm=(ll==1+np.argmax(ss[1:,cv2.CC_STAT_AREA])).astype(np.uint8)
        cs,_=cv2.findContours(fm,cv2.RETR_EXTERNAL,cv2.CHAIN_APPROX_NONE); fm[:]=0; cv2.drawContours(fm,cs,-1,1,-1)
        fm=cv2.resize(fm,(xb-xa,yb_-ya),interpolation=cv2.INTER_LINEAR)
        fm=cv2.morphologyEx(fm,cv2.MORPH_OPEN,np.ones((5,5),np.uint8))
        ys,xs=np.where(fm>0)
        if len(xs)==0: continue
        cx0,cx1,cy0,cy1=xs.min(),xs.max()+1,ys.min(),ys.max()+1
        a=cv2.GaussianBlur(fm.astype(np.float32),(7,7),0)[cy0:cy1,cx0:cx1]
        rgba=np.dstack([roi[cy0:cy1,cx0:cx1],(a*255).astype(np.uint8)])
        cv2.imwrite(f'build/deco-{i}-{j}.png',rgba)
        holes[ya:yb_,xa:xb]|=fm
        # position in mm, nudged so it stays on the page
        mx0,my0=X(xa+cx0),Y(ya+cy0); mw,mh=(cx1-cx0)*sx,(cy1-cy0)*sy
        mx0=min(max(mx0,M+1.5),PW-M-1.5-mw); my0=min(max(my0,M+1.5),PH-M-1.5-mh)
        decotag+=f'<image href="deco-{i}-{j}.png" x="{mx0:.3f}" y="{my0:.3f}" width="{mw:.3f}" height="{mh:.3f}" preserveAspectRatio="none"/>'
    if holes.any():   # remove the originals from the base so a nudged sticker leaves no ghost
        base=cv2.inpaint(img,cv2.dilate(holes,np.ones((9,9),np.uint8)),7,cv2.INPAINT_TELEA)
    img_out=base
    cv2.imwrite(f'build/base-{i}.jpg',img_out,[cv2.IMWRITE_JPEG_QUALITY,93])
    # colours from the measured band
    bgr=pick.astype(int)
    hexc=lambda c:'#%02x%02x%02x'%tuple(int(max(0,min(255,v))) for v in c)
    rgb=bgr[::-1]; base=hexc(rgb); dark=hexc(rgb*0.72); light_=hexc(rgb+(255-rgb)*0.45)
    yt=inner['Ts']; yb=inner['B']
    def grain(horizontal, a0, a1, step):
        out=''
        for k,v in enumerate(np.arange(a0,a1,step)):
            j=(k*37)%11/11.0
            if horizontal: d=f"M -5 {v:.1f} C 90 {v-1.2-j:.1f} 180 {v+1.5:.1f} 260 {v-0.5:.1f} S 380 {v+1.2+j:.1f} 430 {v:.1f}"
            else: d=f"M {v:.1f} -5 C {v-1-j:.1f} 70 {v+1.4:.1f} 160 {v-0.4:.1f} S {v+1+j:.1f} 260 {v:.1f} 305"
            out+=f'<path d="{d}" fill="none" stroke="{light_}" stroke-opacity="{0.35+0.25*j:.2f}" stroke-width="{0.5+(k%3)*0.45:.2f}"/>'
        return out
    streaks=f'<clipPath id="ch{i}"><rect x="{inner["L"]:.1f}" y="-5" width="{inner["R"]-inner["L"]:.1f}" height="310"/></clipPath>'\
            f'<clipPath id="cv{i}"><rect x="-5" y="{yt:.1f}" width="430" height="{yb-yt:.1f}"/></clipPath>'\
            f'<g clip-path="url(#ch{i})">{grain(True,-2,300,3.6)}</g><g clip-path="url(#cv{i})">{grain(False,-2,24,3.6)}{grain(False,396,424,3.6)}</g>'
    vstreaks=''
    svg=f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {PW} {PH}" width="{PW}mm" height="{PH}mm">
<defs>
 <clipPath id="ci{i}"><path d="{path(pin)}"/></clipPath>
 <clipPath id="co{i}"><path d="{path(shape(inner['L']-T-4,inner['R']+T+4,inner['Ts']-T-4,inner['B']+T+4,inner['bL']-T-4,inner['bR']+T+4,inner['Tb']-T-4,R_IN+T+4,RB_IN+T+4,max(1,F_IN-T-4)))}"/></clipPath>
 <clipPath id="cb{i}"><path d="{path(pout)} {path(pin[::-1])}" clip-rule="evenodd"/></clipPath>
</defs>
<rect width="{PW}" height="{PH}" fill="#fff"/>
<g clip-path="url(#ci{i})"><image href="base-{i}.jpg" x="{tx:.3f}" y="{ty:.3f}" width="{w*sx:.3f}" height="{h*sy:.3f}" preserveAspectRatio="none"/></g>
<path d="{path(pout)} {path(pin[::-1])}" fill="{base}" fill-rule="evenodd"/>
<g clip-path="url(#cb{i})">{streaks}{vstreaks}</g>
<path d="{path(pin)}" fill="none" stroke="{dark}" stroke-opacity=".55" stroke-width="0.7"/>
<path d="{path(pout)}" fill="none" stroke="{dark}" stroke-width="1.1"/>
{decotag}
</svg>'''
    pages.append(svg); print(i,base,'stickers',len(merged))
html='''<!doctype html><html><head><meta charset="utf-8"><style>@page{size:420mm 297mm;margin:0}
html,body{margin:0}.p{width:420mm;height:297mm;overflow:hidden;break-after:page}.p:last-child{break-after:auto}svg{display:block}</style></head><body>'''+''.join(f'<div class="p">{s}</div>' for s in pages)+'</body></html>'
open('build/signs.html','w').write(html)
