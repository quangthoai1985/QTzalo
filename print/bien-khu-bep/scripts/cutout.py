# Remove the background (and drop shadow) outside each sign's border, crop tight.
import cv2, numpy as np
for i in range(1,7):
    img=cv2.imread(f'crops/bien-{i}.png'); h,w=img.shape[:2]
    hsv=cv2.cvtColor(img,cv2.COLOR_BGR2HSV)
    bglike=((hsv[...,1]<45)&(hsv[...,2]>150)).astype(np.uint8)
    bglike=cv2.morphologyEx(bglike,cv2.MORPH_OPEN,np.ones((3,3),np.uint8))
    # keep only the bg-like region connected to the image border
    n,lab=cv2.connectedComponents(bglike,connectivity=4)
    edge=set(np.unique(np.concatenate([lab[0],lab[-1],lab[:,0],lab[:,-1]])))-{0}
    outside=np.isin(lab,list(edge))
    sign=(~outside).astype(np.uint8)
    # drop specks, keep largest blob plus attached decorations
    n,lab,stats,_=cv2.connectedComponentsWithStats(sign,connectivity=8)
    big=1+np.argmax(stats[1:,cv2.CC_STAT_AREA]); sign=(lab==big).astype(np.uint8)
    sign=cv2.morphologyEx(sign,cv2.MORPH_CLOSE,np.ones((9,9),np.uint8))
    sign=cv2.erode(sign,np.ones((7,7),np.uint8))
    alpha=cv2.GaussianBlur(sign.astype(np.float32),(5,5),0)
    ys,xs=np.where(sign>0); x0,x1,y0,y1=xs.min(),xs.max()+1,ys.min(),ys.max()+1
    a=alpha[y0:y1,x0:x1,None]; c=img[y0:y1,x0:x1].astype(np.float32)
    out=(c*a+255*(1-a)).astype(np.uint8)
    cv2.imwrite(f'crops/cut-{i}.png',out)
    rgba=np.dstack([img[y0:y1,x0:x1],(a[...,0]*255).astype(np.uint8)]); cv2.imwrite(f'crops/cut-{i}-alpha.png',rgba)
    print(i,(x1-x0,y1-y0),round((x1-x0)/(y1-y0),3))
