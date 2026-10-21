// Original MIT. Curved geometric-optics rays through a weather-derived profile.
import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
struct Settings {
    q0:vec4<f32>,q1:vec4<f32>,q2:vec4<f32>,origin:vec4<f32>,center:vec4<f32>,latitude:vec4<f32>,
    right:vec4<f32>,up:vec4<f32>,forward:vec4<f32>,lens:vec4<f32>,budget:vec4<f32>,
    sun:vec4<f32>,moon:vec4<f32>,moon_light:vec4<f32>,environment:vec4<f32>,
    icrf0:vec4<f32>,icrf1:vec4<f32>,icrf2:vec4<f32>,
    ground_origin:vec4<f32>,east:vec4<f32>,vertical:vec4<f32>,south:vec4<f32>,phase:vec4<f32>,
    region0:array<vec4<f32>,4>,region1:array<vec4<f32>,4>,
    layers:array<vec4<f32>,128>, eclipse:EclipseSettings,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> settings:Settings;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<storage,read> boundaries:array<vec4<f32>>;
fn qmul(p:vec3<f32>)->vec3<f32>{return vec3(dot(settings.q0.xyz,p),dot(settings.q1.xyz,p),dot(settings.q2.xyz,p));}
// Height relative to the true geodetic parallel surfaces, not inflated axes.
fn geometry(p:vec3<f32>)->vec4<f32>{
    let mp=qmul(p);let qp=settings.origin.xyz+mp;
    let f=settings.origin.w+dot(p,2.0*settings.origin.xyz+mp);
    if length(qp)<0.75 {return vec4(0.0,1.0,0.0,-1000000.0);}
    var normal=normalize(qp);var h=0.0;
    for(var i=0;i<5;i+=1){let qn=qmul(normal);let v=dot(normal,qp);
        h=f/(v+sqrt(max(0.0,v*v-f*dot(normal,qn))));normal=normalize(qp-h*qn);}
    return vec4(normal,h);
}
fn index_gradient(h:f32)->vec2<f32>{
    let count=u32(settings.budget.x);
    if h>=settings.budget.y {return vec2(0.0);}
    var lo=0u;var hi=count-1u;
    for(var i=0;i<8;i+=1){if hi<=lo+1u {break;}let mid=(lo+hi)/2u;
        if settings.layers[mid].x<=h {lo=mid;}else{hi=mid;}}
    let knot=settings.layers[lo];return vec2(knot.y+knot.z*(h-knot.x),knot.z);
}
fn knot_bound(gap:f32,velocity:f32,curvature:f32)->f32 {
    let root=sqrt(velocity*velocity+2.0*curvature*gap);
    if velocity>=0.0 {return 2.0*gap/max(root+velocity,1e-20);}
    return (root-velocity)/curvature;
}
// Conservative height excursion to the nearest profile knots in either direction.
// A negative result means the local f32 geometry cannot resolve those layers.
fn layer_step(height:f32,vertical:f32,curvature:f32,band:f32)->f32 {
    let count=u32(settings.budget.x);var lo=0u;var hi=count-1u;
    for(var i=0;i<8;i+=1){if hi<=lo+1u {break;}let mid=(lo+hi)/2u;
        if settings.layers[mid].x<=height {lo=mid;}else{hi=mid;}}
    let first=select(0u,lo-1u,lo>0u);let last=min(lo+1u,count-2u);
    for(var i=first;i<=last;i+=1u){
        if settings.layers[i+1u].x-settings.layers[i].x<4.0*band {return -1.0;}
    }
    var limit=1e30;
    var below=i32(lo);
    if height-settings.layers[lo].x<=band {below-=1;}
    if below>=0 {let gap=height-settings.layers[u32(below)].x;
        if gap>band {limit=min(limit,knot_bound(gap,-vertical,curvature));}}
    var above=lo+1u;
    if settings.layers[above].x-height<=band {above+=1u;}
    if above<count {let gap=settings.layers[above].x-height;
        if gap>band {limit=min(limit,knot_bound(gap,vertical,curvature));}}
    return limit;
}
struct State{p:vec3<f32>,u:vec3<f32>};
fn midpoint(s:State,step:f32)->State{
    let g=geometry(s.p);let ng=index_gradient(g.w);
    let bend=(g.xyz-s.u*dot(s.u,g.xyz))*(ng.y/(1.0+ng.x));
    let mp=s.p+s.u*(step*0.5);let mu=normalize(s.u+bend*(step*0.5));
    let mg=geometry(mp);let mn=index_gradient(mg.w);
    return State(s.p+mu*step,normalize(s.u+(mg.xyz-mu*dot(mu,mg.xyz))*(mn.y/(1.0+mn.x)*step)));
}
fn advance(s:State,step:f32)->State{return midpoint(midpoint(s,step*0.5),step*0.5);}
struct Trace{p:vec3<f32>,u:vec3<f32>,path:f32,status:u32}; //0 sky,1 surface,2 unresolved
fn trace(direction:vec3<f32>)->Trace{
    var state=State(vec3(0.0),direction);var path=0.0;var proposed=2000.0;
    if settings.budget.w<0.5 {return Trace(state.p,state.u,path,2u);}
    if settings.center.w>settings.budget.y {
        let o=-settings.center.xyz;let along=dot(o,direction);let distance=length(cross(o,direction));
        let radius=6378137.0+settings.budget.y;
        if distance>radius || along>=0.0 {return Trace(state.p,state.u,path,0u);}
        let span=sqrt(max(0.0,(radius-distance)*(radius+distance)));
        let lower=max(0.0,-along-span);var a=lower;var b=-along+span;
        for(var i=0;i<40;i+=1){let x=a+(b-a)*0.38196601125;let y=a+(b-a)*0.61803398875;
            if geometry(direction*x).w<geometry(direction*y).w {b=y;}else{a=x;}}
        var upper=(a+b)*0.5;
        if geometry(direction*upper).w>settings.budget.y {return Trace(state.p,state.u,path,0u);}
        var low=lower;
        for(var i=0;i<32;i+=1){let mid=(low+upper)*0.5;
            if geometry(direction*mid).w>settings.budget.y {low=mid;}else{upper=mid;}}
        state.p=direction*upper;path=upper;
    }
    let entry=path;let curvature=settings.budget.z;
    for(var i=0;i<4096;i+=1){
        let g=geometry(state.p);let vertical=dot(state.u,g.xyz);
        if g.w>=settings.budget.y-0.01 && vertical>0.0 {return Trace(state.p,state.u,path,0u);}
        if path-entry>2000000.0 {return Trace(state.p,state.u,path,2u);}
        let disc=sqrt(max(0.0,vertical*vertical+2.0*curvature*max(g.w,0.0)));
        var safe=(vertical+disc)/curvature;
        if vertical<0.0 {safe=2.0*max(g.w,0.0)/max(disc-vertical,1e-20);}
        var step=min(min(proposed,2000.0),max(safe*0.8,0.01));
        // f32 presentation accuracy is scale-dependent; f64 metrology remains independent.
        let tolerance=max(0.002,4e-7*length(state.p));
        if g.w<tolerance && vertical< -1e-8 {step=clamp(2.0*max(g.w,1e-8)/-vertical,0.01,1.0);}
        let knot=layer_step(g.w,vertical,curvature,max(1e-5,4e-7*length(state.p)));
        if knot<0.0 {return Trace(state.p,state.u,path,2u);}
        step=min(step,max(0.01,knot));
        let full=midpoint(state,step);let half=advance(state,step);
        let error=max(length(full.p-half.p),length(full.u-half.u)*step)/3.0;
        if error>tolerance && step>0.01000001 {proposed=step*0.5;continue;}
        if error>tolerance {return Trace(state.p,state.u,path,2u);}
        let next_height=geometry(half.p).w;
        if next_height<=0.0 {
            var low=0.0;var high=step;
            for(var j=0;j<20;j+=1){let mid=(low+high)*0.5;
                if geometry(advance(state,mid).p).w>0.0 {low=mid;}else{high=mid;}}
            let delta=(low+high)*0.5;let end=advance(state,delta);
            return Trace(end.p,end.u,path+delta,1u);
        }
        if g.w<settings.budget.y && next_height>=settings.budget.y {
            var low=0.0;var high=step;
            for(var j=0;j<20;j+=1){let mid=(low+high)*0.5;
                if geometry(advance(state,mid).p).w<settings.budget.y {low=mid;}else{high=mid;}}
            let delta=(low+high)*0.5;let end=advance(state,delta);
            return Trace(end.p,end.u,path+delta,0u);
        }
        state=half;path+=step;proposed=select(step,step*2.0,error<tolerance*0.125);
    }
    return Trace(state.p,state.u,path,2u);
}
fn relative_angles(n:vec3<f32>)->vec2<f32>{
    let a=n.y*settings.latitude.y+n.z*settings.latitude.x;
    let h=length(vec2(a,n.x));var d=h-a;
    if a>=0.0 && h+a>0.0 {d=n.x*n.x/(h+a);}
    return vec2(atan2(-n.z-d*settings.latitude.x,n.y+d*settings.latitude.y),atan2(n.x,a));
}
fn cross2(a:vec2<f32>,b:vec2<f32>)->f32{return a.x*b.y-a.y*b.x;}
fn water_at(n:vec3<f32>)->bool{
    let angles=relative_angles(n);
    for(var region=0u;region<4u;region+=1u){
        let a=settings.region0[region];let b=settings.region1[region];if b.z<1.0 {continue;}
        let point=vec2((a.x-angles.x)*a.z,(a.y+angles.y)*a.w);
        if any(point<vec2(0.0)) || point.x>3.0*b.y || point.y>3.0*b.x {continue;}
        let cell=min(vec2<u32>(point/3.0),vec2<u32>(b.yx)-vec2(1u));
        let bin=boundaries[u32(b.z)+cell.x*u32(b.x)+cell.y];
        for(var i=0u;i<u32(bin.y);i+=1u){
            let id=u32(boundaries[u32(bin.x)+i].x);let t=boundaries[id];let v=boundaries[id+1u];
            let p0=t.xy;let p1=t.zw;let p2=v.xy;
            let signs=vec3(cross2(p1-p0,point-p0),cross2(p2-p1,point-p1),cross2(p0-p2,point-p2));
            // Signed edge distance, not a fixed area tolerance on skinny faces.
            let epsilon=0.0002*vec3(length(p1-p0),length(p2-p1),length(p0-p2));
            if all(signs>=-epsilon) || all(signs<=epsilon) {return v.z>0.5;}
        }
    }
    return false;
}
fn sky_color(direction:vec3<f32>)->vec3<f32>{
    let t=pow(max(direction.y,0.0),0.55);let daylight=settings.environment.x*settings.environment.y;
    return mix(vec3(0.34,0.48,0.65),vec3(0.055,0.20,0.43),t)*daylight+vec3(0.0004,0.0007,0.0016);
}
fn sphere_hit(o:vec3<f32>,d:vec3<f32>,body:vec4<f32>)->f32{
    if body.w<=0.0 {return -1.0;}let relative=body.xyz-o;let along=dot(relative,d);
    let closest=length(cross(relative,d));if along<=0.0 || closest>body.w {return -1.0;}
    return along-sqrt(max(0.0,(body.w-closest)*(body.w+closest)));
}
fn hash_cell(v:vec3<i32>)->f32{
    var h=(bitcast<u32>(v.x)*1597334677u) ^ (bitcast<u32>(v.y)*3812015801u) ^ (bitcast<u32>(v.z)*2798796415u);
    h=(h^(h>>16u))*2246822519u;h=h^(h>>13u);return f32(h&65535u)/65535.0;
}
fn radiance(ray:Trace)->vec3<f32>{
    if ray.status==2u {return vec3(0.24,0.015,0.18);} // explicit unresolved diagnostic
    if ray.status==0u {
        let sun_hit=sphere_hit(ray.p,ray.u,settings.sun);let moon_hit=sphere_hit(ray.p,ray.u,settings.moon);
        if moon_hit>0.0 && (sun_hit<0.0 || moon_hit<sun_hit){
            let normal=normalize(ray.p+ray.u*moon_hit-settings.moon.xyz);
            let brightness=0.006+0.78*max(dot(normal,settings.moon_light.xyz),0.0);
            return vec3(0.76,0.75,0.72)*brightness;
        }
        if sun_hit>0.0 {return vec3(1.0,0.92,0.68);}
        var color=sky_color(ray.u);
        // Original illustrative angular stars; neither catalogue nor photometry.
        let inertial=vec3(dot(settings.icrf0.xyz,ray.u),dot(settings.icrf1.xyz,ray.u),dot(settings.icrf2.xyz,ray.u));
        let axis=max(max(abs(inertial.x),abs(inertial.y)),abs(inertial.z));
        let cube=inertial/max(axis,1e-8)*180.0;let cell=vec3<i32>(floor(cube));
        let star=hash_cell(cell);let fraction=fract(cube);let distance=min(min(length(fraction.xy-0.5),length(fraction.xz-0.5)),length(fraction.yz-0.5));
        let star_radius=min(0.11,180.0/max(settings.lens.w,1.0));
        color+=vec3(select(0.0,0.6,star>0.9985 && distance<star_radius))*settings.environment.z;
        return color;
    }
    let geometry_at=geometry(ray.p);let normal=geometry_at.xyz;
    let sun=normalize(select(vec3(0.0,1.0,0.0),settings.sun.xyz-ray.p,settings.sun.w>0.0));
    let visible=eclipse_solar_visibility(ray.p,settings.eclipse);
    let direct=select(0.0,max(dot(normal,sun),0.0)*visible,settings.sun.w>0.0);
    let fill=settings.environment.x*(0.04+0.96*sqrt(visible));
    let ecef=settings.ground_origin.xyz+settings.east.xyz*ray.p.x+settings.vertical.xyz*ray.p.y+settings.south.xyz*ray.p.z;
    let point=ecef-floor(ecef/4096.0)*4096.0;
    // Grazing surface projection stretches a pixel footprint. This local-plane
    // heuristic suppresses shimmer; it is not a caustic/Jacobian error bound.
    let footprint=ray.path/(max(settings.lens.w,1.0)*max(abs(dot(ray.u,normal)),1e-6));
    var color=vec3(0.18,0.27,0.11)*(0.08+1.35*direct+0.32*fill);
    if water_at(normal){
        let fade=1.0-smoothstep(0.5,5.0,footprint);
        let k=6.28318530718/16.0;
        let phase=dot(point,vec3(1.0,2.0,0.0))*k;
        let derivative=cos(phase)*settings.phase.y-sin(phase)*settings.phase.x;
        let ecef_slope=vec3(1.0,2.0,0.0)*0.018*derivative*fade;
        let local_slope=vec3(dot(ecef_slope,settings.east.xyz),dot(ecef_slope,settings.vertical.xyz),dot(ecef_slope,settings.south.xyz));
        let n=normalize(normal-(local_slope-normal*dot(local_slope,normal)));
        let reflected=reflect(ray.u,n);let fresnel=0.02+0.98*pow(1.0-max(dot(-ray.u,n),0.0),5.0);
        let highlight=select(0.0,pow(max(dot(reflect(-sun,n),-ray.u),0.0),180.0)*visible,settings.sun.w>0.0);
        color=vec3(0.012,0.055,0.070)*(0.10+direct+0.3*fill)
            +sky_color(reflected)*fresnel+vec3(1.0,0.90,0.70)*highlight*0.8;
    }else{
        let detail=(sin(point.x*0.392699)*sin(point.y*0.392699)*sin(point.z*0.392699));
        color*=1.0+0.035*detail*(1.0-smoothstep(0.5,3.0,footprint));
    }
    // Original illustrative haze, distinct from the physical ray index model.
    let haze=(1.0-exp(-ray.path*0.000018))*0.55*settings.environment.x;
    return mix(color,sky_color(normalize(ray.u+normal*0.3)),haze);
}
@fragment
fn fragment(in:VertexOutput)->@location(0) vec4<f32>{
    let xy=vec2((2.0*in.uv.x-1.0)*settings.lens.x,1.0-2.0*in.uv.y);
    var optical=normalize(vec3(xy*tan(settings.lens.y),1.0));
    if settings.lens.z>0.5 {
        let radius=length(xy);if radius>1.0 {return vec4(0.002,0.004,0.008,1.0);}
        let theta=radius*settings.lens.y;let scale=select(settings.lens.y,sin(theta)/max(radius,1e-8),radius>1e-8);
        optical=vec3(xy*scale,cos(theta));
    }
    let direction=normalize(settings.right.xyz*optical.x+settings.up.xyz*optical.y+settings.forward.xyz*optical.z);
    return vec4(radiance(trace(direction)),1.0);
}
