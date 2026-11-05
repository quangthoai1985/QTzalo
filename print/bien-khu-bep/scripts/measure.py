# Measure each sign's inner cream panel (in 4x crop pixels) and fit a parametric frame shape.
import cv2, numpy as np, json
res={}
for i in range(1,7):
    img=cv2.imread(f'crops/bien-{i}.png'); h,w=img.shape[:2]
    small=cv2.resize(img,(w//4,h//4),interpolation=cv2.INTER_AREA)
    hsv=cv2.cvtColor(small,cv2.COLOR_BGR2HSV).astype(int)
    H,S,V=hsv[...,0],hsv[...,1],hsv[...,2]
    cream=((V>205)&(S>=10)&(S<70)&(H>=8)&(H<=32)).astype(np.uint8)
    n,lab,st,_=cv2.connectedComponentsWithStats(cream,connectivity=4)
    big=1+np.argmax(st[1:,cv2.CC_STAT_AREA]); m=(lab==big).astype(np.uint8)
    m=cv2.morphologyEx(m,cv2.MORPH_CLOSE,np.ones((25,25),np.uint8))
    cnts,_=cv2.findContours(m,cv2.RETR_EXTERNAL,cv2.CHAIN_APPROX_NONE)
    m2=np.zeros_like(m); cv2.drawContours(m2,cnts,-1,1,-1); m=m2
    ys,xs=np.where(m>0); mh,mw=m.shape
    rows=np.arange(int(ys.min()+0.25*(ys.max()-ys.min())), int(ys.min()+0.62*(ys.max()-ys.min())))
    left=[np.where(m[y])[0].min() for y in rows]; right=[np.where(m[y])[0].max() for y in rows]
    cols=np.arange(xs.min(),xs.max()+1)
    top=np.array([np.where(m[:,x])[0].min() if m[:,x].any() else mh for x in cols])
    bot=np.array([np.where(m[:,x])[0].max() if m[:,x].any() else 0 for x in cols])
    W=len(cols)
    L=np.percentile(left,85); R=np.percentile(right,15); B=np.percentile(bot[int(.2*W):int(.8*W)],10)
    Ts=np.percentile(np.r_[top[int(.08*W):int(.2*W)],top[int(.8*W):int(.92*W)]],85)
    Tb=np.percentile(top[int(.4*W):int(.6*W)],25)
    mid=(Ts+Tb)/2
    idx=np.where(top<mid)[0]; bL=cols[idx.min()]; bR=cols[idx.max()]
    res[i]={k:float(v)*4 for k,v in dict(L=L,R=R,Ts=Ts,B=B,Tb=Tb,bL=bL,bR=bR).items()}
    print(i,{k:round(v) for k,v in res[i].items()})
    dbg=small.copy(); cv2.drawContours(dbg,cnts,-1,(255,0,255),2)
    for x in (L,R,bL,bR): cv2.line(dbg,(int(x),0),(int(x),mh),(0,0,255),1)
    for y in (Ts,B,Tb): cv2.line(dbg,(0,int(y)),(mw,int(y)),(0,0,255),1)
    cv2.imwrite(f'dbg-{i}.png',dbg)
json.dump(res,open('frames.json','w'),indent=1)
