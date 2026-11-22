"""Host-only Gaussian appearance and shade-encoding references.

Not a firmware memory/performance implementation. Foreground is composed later.
"""
import math,time

def gaussian(p,w,h,sigma=14):
    radius=math.ceil(sigma*3)
    weights=[math.exp(-(d*d)/(2*sigma*sigma)) for d in range(-radius,radius+1)]
    total=sum(weights);weights=[v/total for v in weights]
    xs=[[min(w-1,max(0,x+d)) for d in range(-radius,radius+1)] for x in range(w)]
    ys=[[min(h-1,max(0,y+d)) for d in range(-radius,radius+1)] for y in range(h)]
    horizontal=[0.]*(w*h)
    for y in range(h):
        row=p[y*w:(y+1)*w]
        if all(v==255 for v in row):horizontal[y*w:(y+1)*w]=[255.]*w;continue
        for x in range(w):horizontal[y*w+x]=sum(row[j]*k for j,k in zip(xs[x],weights))
    output=bytearray(w*h)
    for x in range(w):
        column=horizontal[x::w]
        if all(v==255 for v in column):
            for y in range(h):output[y*w+x]=255
            continue
        for y in range(h):output[y*w+x]=round(sum(column[j]*k for j,k in zip(ys[y],weights)))
    return output

def diffuse(p,w,h,atkinson=False,levels=2):
    # Serpentine processing avoids giving every row the same left-to-right direction.
    work=[float(v) for v in p];out=bytearray(w*h)
    for y in range(h):
        sign=1 if y%2==0 else -1
        order=range(w) if sign==1 else range(w-1,-1,-1)
        for x in order:
            i=y*w+x;old=work[i]
            new=min(3,max(0,round(old/85)))*85 if levels==4 else 255 if old>=127.5 else 0
            out[i]=new;error=old-new
            taps=[(sign,0,1/8),(2*sign,0,1/8),(-sign,1,1/8),(0,1,1/8),(sign,1,1/8),(0,2,1/8)] if atkinson else [(sign,0,7/16),(-sign,1,3/16),(0,1,5/16),(sign,1,1/16)]
            for dx,dy,k in taps:
                if 0<=x+dx<w and y+dy<h:work[(y+dy)*w+x+dx]+=error*k
    return out

def build(p,w,h):
    started=time.perf_counter();soft=gaussian(p,w,h)
    veil=bytearray(round(255*.10+v*.90) for v in soft)
    bayer=[[0,8,2,10],[12,4,14,6],[3,11,1,9],[15,7,13,5]]
    result={
      'gaussian-reference':veil,
      'gray4-candidate':bytearray(min(3,round(v/85))*85 for v in veil),
      'gray4-diffusion':diffuse(veil,w,h,levels=4),
      'fs-bw':diffuse(veil,w,h),
      'atkinson-bw':diffuse(veil,w,h,True),
      'bayer-bw':bytearray(255 if v>(bayer[(i//w)%4][i%4]+.5)*255/16 else 0 for i,v in enumerate(veil))}
    metrics={'sigma_pixels':14,'kernel_radius_pixels':42,'white_veil_fraction':.10,'gaussian_min':min(soft),'veiled_min':min(veil),'host_elapsed_seconds':round(time.perf_counter()-started,3),'limits':'Host reference only; no C3 latency, scratch-memory or panel timing claim.','primary_method':'Serpentine Floyd–Steinberg, 1 bit. Gaussian filter is separate from encoding.'}
    return result,metrics
