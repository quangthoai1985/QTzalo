import cv2, sys
src='/root/.claude/uploads/1244aada-0123-58ab-87a8-8a3e2420fcc9/4d7ecd8f-image.jpg'
img=cv2.imread(src)
P=14
boxes=[(28,57,880,687),(919,64,1662,693),(1704,63,2525,695),(25,732,834,1356),(884,732,1677,1356),(1699,732,2530,1356)]
sr=cv2.dnn_superres.DnnSuperResImpl_create(); sr.readModel('models/EDSR_x4.pb'); sr.setModel('edsr',4)
for i,(x0,y0,x1,y1) in enumerate(boxes,1):
    c=img[max(0,y0-P):y1+P, max(0,x0-P):x1+P]
    up=sr.upsample(c)
    cv2.imwrite(f'crops/bien-{i}.png', up); print(i, c.shape, up.shape, flush=True)
