#!/usr/bin/env python3
"""Calibrate LaserCraft camera lens from multiple chessboard photographs (offline)."""
import argparse, glob, json, sys
from pathlib import Path

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('images',nargs='+');p.add_argument('--columns',type=int,required=True);p.add_argument('--rows',type=int,required=True)
    p.add_argument('--square-mm',type=float,required=True);p.add_argument('--output',default='lasercraft-lens.json');p.add_argument('--max-rms-px',type=float,default=1.0)
    a=p.parse_args()
    if min(a.columns,a.rows)<3 or a.square_mm<=0 or a.max_rms_px<=0:p.error('Invalid target parameters')
    try:import cv2,numpy as np
    except ImportError:p.error('Install opencv-python and numpy')
    paths=sorted(set(f for pat in a.images for f in glob.glob(pat)))
    obj=[];image=[];used=[];rejected=[];size=None
    grid=np.zeros((a.columns*a.rows,3),np.float32);grid[:,:2]=np.mgrid[0:a.columns,0:a.rows].T.reshape(-1,2)*a.square_mm
    for path in paths:
        frame=cv2.imread(path)
        if frame is None:rejected.append([path,'unreadable']);continue
        wh=(frame.shape[1],frame.shape[0])
        if size is None:size=wh
        if wh!=size:rejected.append([path,'resolution mismatch']);continue
        gray=cv2.cvtColor(frame,cv2.COLOR_BGR2GRAY)
        found,corners=cv2.findChessboardCorners(gray,(a.columns,a.rows),cv2.CALIB_CB_ADAPTIVE_THRESH|cv2.CALIB_CB_NORMALIZE_IMAGE)
        if not found:rejected.append([path,'corners not found']);continue
        corners=cv2.cornerSubPix(gray,corners,(11,11),(-1,-1),(cv2.TERM_CRITERIA_EPS+cv2.TERM_CRITERIA_MAX_ITER,40,0.001))
        obj.append(grid.copy());image.append(corners);used.append(path)
    if len(used)<8:p.error(f'Need at least 8 detected views; found {len(used)}')
    rms,K,d,rvec,tvec=cv2.calibrateCamera(obj,image,size,None,None)
    w,h=size
    errors=[]
    for path,points,observed,r,t in zip(used,obj,image,rvec,tvec):
        projected,_=cv2.projectPoints(points,r,t,K,d)
        delta=np.linalg.norm(projected.reshape(-1,2)-observed.reshape(-1,2),axis=1)
        errors.append({'image':path,'rms_px':float(np.sqrt(np.mean(delta**2))),'max_px':float(delta.max())})
    result={'format':'lasercraft-intrinsics-v1','image_size':[w,h],'views':len(used),'rms_px':float(rms),
            'accepted':bool(np.isfinite(rms) and rms<=a.max_rms_px),'threshold_px':a.max_rms_px,
            'lens':[float(d.flat[0]),float(d.flat[1]),float(d.flat[2]),float(d.flat[3]),float(K[0,2]/w),float(K[1,2]/h),float(K[0,0]/w),float(K[1,1]/h)],
            'k3':float(d.flat[4]) if d.size>4 else 0.0,'per_view':errors,'rejected':rejected}
    Path(a.output).write_text(json.dumps(result,indent=2))
    print(f"Views={len(used)} RMS={rms:.3f}px accepted={result['accepted']} output={a.output}")
    return 0 if result['accepted'] else 2
if __name__=='__main__':sys.exit(main())
