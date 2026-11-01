"""Offline only: acquire OpenF1 history, create causal features, train ONNX, report held-out races.

Private raw cache and Parquet stay under ignored data/. Never run acquisition alongside live ingest
on the same unauthenticated source quota. Public API: https://openf1.org/docs/.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime

FEATURES=["baseline","tyre_age","compound","track_temperature","interval","lap","stint"]
COMPOUNDS={"SOFT":0,"MEDIUM":1,"HARD":2,"INTERMEDIATE":3,"WET":4}

def clean_lap(lap):
    value=lap.get("lap_duration")
    return (isinstance(value,(int,float)) and math.isfinite(value) and value>0
            and not lap.get("is_pit_out_lap",False) and not lap.get("pit_in",False)
            and lap.get("status")=="GREEN"
            and (lap.get("interval") is None or lap["interval"]==0 or lap["interval"]>=1.5))

def grouped_split(groups):
    races=sorted(set(groups))
    if len(races)<2:raise ValueError("At least two separate races are required for held-out evaluation")
    cut=max(1,min(len(races)-1,int(len(races)*0.8)))
    return races[:cut],races[cut:]

def score_forecasts(records):
    errors=[abs(row["prediction"]-row["actual"]) for row in records if row.get("actual") is not None]
    return {"samples":len(errors),"mae":sum(errors)/len(errors) if errors else None}

def stamp(value):
    try:return datetime.fromisoformat(value.replace("Z","+00:00")).timestamp()
    except (ValueError,TypeError,AttributeError):return None

def track_status(row,previous):
    text=row.get("message","").upper()
    if "VIRTUAL SAFETY CAR DEPLOYED" in text:return "VSC"
    if "SAFETY CAR DEPLOYED" in text:return "SC"
    if row.get("flag")=="RED" and row.get("scope") in (None,"Track"):return "RED"
    if "TRACK CLEAR" in text or "GREEN LIGHT" in text or "VIRTUAL SAFETY CAR ENDED" in text or (row.get("flag")=="GREEN" and row.get("scope")=="Track"):return "GREEN"
    return previous

def stint_baseline(samples,next_lap):
    """Same causal weak-prior/posterior and changepoint rule as pitwall-predict/analysis.rs."""
    def fit(points):
        n=len(points);mx=sum(x for x,_ in points)/n;my=sum(y for _,y in points)/n
        xx=sum((x-mx)**2 for x,_ in points);slope=sum((x-mx)*(y-my) for x,y in points)/(xx+2)
        sse=sum((y-my-slope*(x-mx))**2 for x,y in points)
        variance=(0.0324+0.5*(sse+2*slope*slope))/(1+(n-1)/2)
        return dict(n=n,mx=mx,my=my,xx=xx,slope=slope,sse=sse,variance=variance)
    def mean(p,x):return p["my"]+p["slope"]*(x-p["mx"])
    def sd(p,x):return max(0.12,math.sqrt(p["variance"]*(1+1/p["n"]+(x-p["mx"])**2/(p["xx"]+2))))
    posterior=fit(samples);best_cost=math.inf;cliff=None
    for split in range(3,len(samples)-2):
        before=fit(samples[:split]);after=fit(samples[split:]);cost=before["sse"]+after["sse"]
        evidence=len(samples)*math.log((posterior["sse"]+0.0324)/(cost+0.0324))
        sustained=all(y-mean(before,x)>max(0.6,3*sd(before,x)) for x,y in samples[-3:])
        if cost<best_cost and sustained and evidence>3*math.log(len(samples))+6:best_cost=cost;cliff=split
    if cliff is not None:posterior=fit(samples[cliff:])
    return mean(posterior,next_lap)-next_lap*0.035,posterior["slope"]

class Source:
    def __init__(self,root):self.root=root;self.last=0;root.mkdir(parents=True,exist_ok=True)
    def get(self,endpoint,**query):
        encoded=urllib.parse.urlencode(query)
        filename=self.root/f"{endpoint}-{hashlib.sha256(encoded.encode()).hexdigest()[:20]}.json"
        if filename.exists():return json.loads(filename.read_text(encoding="utf-8"))
        url=f"https://api.openf1.org/v1/{endpoint}?{encoded}"
        for attempt in range(5):
            time.sleep(max(0,2.2-(time.monotonic()-self.last)));self.last=time.monotonic()
            try:
                request=urllib.request.Request(url,headers={"User-Agent":"Agon non-commercial educational backtest"})
                try:
                    with urllib.request.urlopen(request,timeout=60) as response:rows=decode_response(response.status,response.read(64*1024*1024))
                except urllib.error.HTTPError as error:
                    if error.code!=404:raise
                    rows=decode_response(error.code,error.read(65536))
                if not isinstance(rows,list):raise ValueError("Unexpected OpenF1 schema")
                temporary=filename.with_suffix(".tmp");temporary.write_text(json.dumps(rows),encoding="utf-8");temporary.replace(filename)
                return rows
            except (urllib.error.URLError,TimeoutError) as error:
                if attempt==4:raise
                print(f"Retry {endpoint}: {type(error).__name__}",flush=True);time.sleep(5*(attempt+1))

def decode_response(status,body):
    value=json.loads(body)
    if status==404 and isinstance(value,dict) and value.get("detail")=="No results found.":return []
    if status<200 or status>=300:raise ValueError(f"OpenF1 HTTP {status}")
    if not isinstance(value,list):raise ValueError("Unexpected OpenF1 schema")
    return value

def acquire(root,years,limit):
    source=Source(root/"raw")
    sessions=[]
    for year in years:sessions.extend(row for row in source.get("sessions",year=year) if row.get("session_name")=="Race" and not row.get("is_cancelled",False))
    sessions.sort(key=lambda s:s.get("date_start",""))
    if limit:sessions=sessions[:limit]
    archives=root/"races";archives.mkdir(parents=True,exist_ok=True)
    for index,session in enumerate(sessions):
        key=session["session_key"];target=archives/f"{key}.json"
        if target.exists():continue
        print(f"Acquire {index+1}/{len(sessions)}: {key} {session.get('location')} {session.get('date_start')}",flush=True)
        archive={"session":session}
        for endpoint in ["laps","stints","pit","weather","race_control","intervals"]:archive[endpoint]=source.get(endpoint,session_key=key)
        temporary=target.with_suffix(".tmp");temporary.write_text(json.dumps(archive),encoding="utf-8");temporary.replace(target)
    return len(sessions)

def prepare(root):
    import numpy as np
    import pandas as pd
    rows=[]
    for path in sorted((root/"races").glob("*.json")):
        archive=json.loads(path.read_text(encoding="utf-8"));session=archive["session"];race=session["session_key"]
        controls=sorted([(stamp(row.get("date")),row) for row in archive["race_control"] if stamp(row.get("date")) is not None],key=lambda x:x[0])
        weather=sorted([(stamp(row.get("date")),row) for row in archive["weather"] if stamp(row.get("date")) is not None],key=lambda x:x[0])
        drivers=sorted(set(row["driver_number"] for row in archive["laps"]))
        for driver in drivers:
            laps=sorted([dict(row) for row in archive["laps"] if row["driver_number"]==driver],key=lambda row:row["lap_number"])
            stints=[row for row in archive["stints"] if row["driver_number"]==driver]
            pits={row["lap_number"] for row in archive["pit"] if row["driver_number"]==driver}
            intervals=sorted([(stamp(row.get("date")),row) for row in archive["intervals"] if row["driver_number"]==driver and stamp(row.get("date")) is not None],key=lambda x:x[0])
            clean=[];last_stint=None
            for i,lap in enumerate(laps):
                start=stamp(lap.get("date_start"));duration=lap.get("lap_duration")
                if start is None or not isinstance(duration,(int,float)):continue
                completed=start+duration
                status="UNKNOWN"
                for date,control in controls:
                    if date>completed:break
                    status=track_status(control,status)
                lap["status"]=status;lap["pit_in"]=lap["lap_number"] in pits
                previous=[row for date,row in intervals if date<=completed]
                interval=previous[-1].get("interval") if previous else None
                lap["interval"]=interval if isinstance(interval,(int,float)) else None
                stint=next((st for st in reversed(stints) if st.get("lap_start",999)<=lap["lap_number"]),None)
                if stint is None:continue
                if stint.get("stint_number")!=last_stint:clean=[];last_stint=stint.get("stint_number")
                if not clean_lap(lap):continue
                clean.append(lap)
                if len(clean)<3 or i+1>=len(laps):continue
                baseline,slope=stint_baseline([(l["lap_number"],l["lap_duration"]+l["lap_number"]*0.035) for l in clean],lap["lap_number"]+1)
                age=stint.get("tyre_age_at_start",0)+lap["lap_number"]-stint["lap_start"]+1
                temps=[w.get("track_temperature") for date,w in weather if date<=completed]
                target=laps[i+1];target_duration=target.get("lap_duration")
                target_start=stamp(target.get("date_start"));target_status=status
                if target_start is not None and isinstance(target_duration,(int,float)):
                    for date,control in controls:
                        if date>target_start+target_duration:break
                        target_status=track_status(control,target_status)
                target_intervals=[record.get("interval") for date,record in intervals if target_start is not None and isinstance(target_duration,(int,float)) and date<=target_start+target_duration]
                target_interval=target_intervals[-1] if target_intervals else None
                target["status"]=target_status;target["interval"]=target_interval if isinstance(target_interval,(int,float)) else None;target["pit_in"]=target["lap_number"] in pits
                pace_residual=float(target_duration-baseline) if target["lap_number"]==lap["lap_number"]+1 and clean_lap(target) else None
                rows.append(dict(race=race,driver=driver,issued_lap=lap["lap_number"],issued_at=completed,degradation=slope,
                    baseline=baseline,tyre_age=age,compound=COMPOUNDS.get(stint.get("compound"),-1),track_temperature=temps[-1] if temps else None,
                    interval=lap["interval"],lap=lap["lap_number"],stint=last_stint,residual=pace_residual,
                    hazard=int(lap["lap_number"]+1 in pits) if archive["pit"] else None,target_duration=target_duration if pace_residual is not None else None))
    if not rows:raise ValueError("No eligible causal samples; acquire more race data")
    frame=pd.DataFrame(rows);frame.to_parquet(root/"features.parquet",index=False);return frame

def train(root,output):
    import numpy as np
    import pandas as pd
    import lightgbm as lgb
    import onnxmltools
    from onnxmltools.convert.common.data_types import FloatTensorType
    from sklearn.metrics import mean_absolute_error,brier_score_loss
    from sklearn.model_selection import GroupKFold,cross_val_score
    frame=prepare(root);train_races,test_races=grouped_split(frame.race.tolist())
    if len(train_races)<3:raise ValueError("At least four races are required for train/calibration/holdout splits")
    calibration_races=train_races[-max(1,len(train_races)//5):];train_races=[r for r in train_races if r not in calibration_races]
    training=frame[frame.race.isin(train_races)];testing=frame[frame.race.isin(test_races)];calibration=frame[frame.race.isin(calibration_races)&frame.residual.notna()]
    fit=training[training.residual.notna()];validation=testing[testing.residual.notna()]
    if len(fit)<100 or len(validation)<20:raise ValueError("Too few held-out clean laps to train/evaluate")
    pace=lgb.LGBMRegressor(n_estimators=80,num_leaves=15,min_child_samples=30,verbosity=-1,random_state=42,n_jobs=2)
    pace.fit(fit[FEATURES],fit.residual)
    hazard=lgb.LGBMClassifier(n_estimators=80,num_leaves=15,min_child_samples=30,verbosity=-1,random_state=42,n_jobs=2)
    hazard_training=training[training.hazard.notna()];hazard_testing=testing[testing.hazard.notna()]
    if hazard_training.hazard.nunique()<2:raise ValueError("No positive pit events in training split")
    hazard.fit(hazard_training[FEATURES],hazard_training.hazard)
    predicted=validation.baseline.to_numpy()+pace.predict(validation[FEATURES]);errors=validation.target_duration.to_numpy()-predicted
    calibration_errors=calibration.target_duration.to_numpy()-(calibration.baseline.to_numpy()+pace.predict(calibration[FEATURES]))
    width=float(np.quantile(np.abs(calibration_errors),0.8))
    output.mkdir(parents=True,exist_ok=True)
    for name,model in [("pace",pace),("hazard",hazard)]:
        options={"zipmap":False} if name=="hazard" else {}
        converted=onnxmltools.convert_lightgbm(model,initial_types=[("input",FloatTensorType([None,len(FEATURES)]))],target_opset=15,**options)
        (output/f"{name}.onnx").write_bytes(converted.SerializeToString())
    import onnxruntime as ort
    x=validation[FEATURES].iloc[:8].to_numpy(dtype=np.float32)
    onnx_pace=ort.InferenceSession(str(output/"pace.onnx"),providers=["CPUExecutionProvider"]).run(None,{"input":x})[0].reshape(-1)
    parity=float(np.max(np.abs(onnx_pace-pace.predict(validation[FEATURES].iloc[:8]))))
    if parity>0.001:raise ValueError(f"ONNX export parity failed: {parity}")
    actual_years=sorted({json.loads(path.read_text(encoding="utf-8"))["session"]["year"] for path in (root/"races").glob("*.json")})
    version="openf1-lgbm-v2-"+hashlib.sha256((root/"features.parquet").read_bytes()).hexdigest()[:10]
    cv=GroupKFold(n_splits=min(5,fit.race.nunique()))
    scores=-cross_val_score(lgb.LGBMRegressor(n_estimators=80,num_leaves=15,min_child_samples=30,verbosity=-1,random_state=42,n_jobs=2),fit[FEATURES],fit.residual,groups=fit.race,cv=cv,scoring="neg_mean_absolute_error")
    indexed=frame.set_index(["race","driver","issued_lap"]);five_errors=[]
    for (_,row),next_prediction in zip(validation.iterrows(),predicted):
        for h in range(1,6):
            key=(row.race,row.driver,row.issued_lap+h-1)
            if key in indexed.index:
                target=indexed.loc[key,"target_duration"]
                if pd.notna(target):five_errors.append(abs(float(target)-(next_prediction+(h-1)*(row.degradation-0.035))))
    pit_cases=[];pit_candidates=0
    for path in (root/"races").glob("*.json"):
        archive=json.loads(path.read_text(encoding="utf-8"));race=archive["session"]["session_key"]
        if race not in test_races:continue
        for stop in archive["pit"]:
            lap=stop.get("lap_number",0)
            if lap<=10:continue
            pit_candidates+=1
            candidate=testing[(testing.race==race)&(testing.driver==stop["driver_number"])&(testing.issued_lap==lap-10)]
            if not candidate.empty:pit_cases.append((candidate.iloc[0],lap))
    simulations=[]
    for case,_ in pit_cases:
        for h in range(30):
            features=case[FEATURES].to_dict();features["tyre_age"]+=h;features["lap"]+=h;simulations.append(features)
    pit_hits=[]
    if simulations:
        hazards=hazard.predict_proba(pd.DataFrame(simulations)[FEATURES])[:,1]
        for i,(case,actual) in enumerate(pit_cases):
            survival=1.0;mass=[]
            for probability in hazards[i*30:(i+1)*30]:mass.append(survival*probability);survival*=1-probability
            forecast=int(case.issued_lap)+1+int(np.argmax(mass));pit_hits.append(abs(forecast-actual)<=2)
    report={"model":version,"baseline_model":"bayesian-changepoint-v2","features":FEATURES,"training_years":actual_years,"train_races":train_races,"calibration_races":calibration_races,"held_out_races":test_races,
        "train_samples":len(training),"pace_samples":len(validation),"pace_mae":float(mean_absolute_error(validation.target_duration,predicted)),
        "pace_p10_p90_half_width":width,"band_coverage":float(np.mean(np.abs(errors)<=width)),
        "hazard_brier":float(brier_score_loss(hazard_testing.hazard,hazard.predict_proba(hazard_testing[FEATURES])[:,1])) if len(hazard_testing) else None,
        "hazard_train_races":sorted(hazard_training.race.unique().tolist()),
        "pace_1_to_5_mae":float(np.mean(five_errors)) if five_errors else None,"pace_1_to_5_samples":len(five_errors),
        "onnx_parity_max_error":parity,"grouped_cv_residual_mae":scores.tolist(),
        "parity_case":{"features":[float(v) if np.isfinite(v) else None for v in x[0]],"residual":float(onnx_pace[0])},
        "artifact_sha256":{name:hashlib.sha256((output/name).read_bytes()).hexdigest() for name in ["pace.onnx","hazard.onnx"]},
        "pit_horizon_10_accuracy":float(np.mean(pit_hits)) if pit_hits else None,"pit_samples":len(pit_hits),"pit_candidates":pit_candidates,"pit_forecast_coverage":len(pit_hits)/pit_candidates if pit_candidates else None,"finish_half_distance_accuracy":None,
        "limitations":["Held-out causal feature backtest; independent Rust replay parity/regression runs separately. Finish scoring needs pre-known scheduled race distance.","Calibration races are separate from training/holdout. Seven available causal features are used; unavailable track pit-loss and team-history features are not fabricated.","Races without source pit records are excluded from hazard labels rather than treated as zero pit stops. Pit score reports only forecasts available exactly ten laps before a recorded stop; see coverage."]}
    (output/"manifest.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
    (root/"backtest-report.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
    print(json.dumps(report,indent=2),flush=True)

def main():
    parser=argparse.ArgumentParser();parser.add_argument("command",choices=["acquire","prepare","train"]);parser.add_argument("--root",type=Path,default=Path("data/training"));parser.add_argument("--years",type=int,nargs="+",default=[2023,2024,2025]);parser.add_argument("--limit",type=int);parser.add_argument("--output",type=Path,default=Path("models"));args=parser.parse_args();args.root.mkdir(parents=True,exist_ok=True)
    if args.command=="acquire":acquire(args.root,args.years,args.limit)
    elif args.command=="prepare":print(f"Prepared {len(prepare(args.root))} rows")
    else:train(args.root,args.output)

if __name__=="__main__":main()
