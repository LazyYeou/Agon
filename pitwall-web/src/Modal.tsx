import {useEffect,useRef,type ReactNode} from 'react';
export default function Modal({label,onClose,children,drawer=false}:{label:string;onClose:()=>void;children:ReactNode;drawer?:boolean}){
  const ref=useRef<HTMLDialogElement>(null);
  useEffect(()=>{const previous=document.activeElement as HTMLElement|null;ref.current?.showModal();return()=>{ref.current?.close();previous?.focus();};},[]);
  return <dialog ref={ref} aria-label={label} className={drawer?'drawer':''} onCancel={e=>{e.preventDefault();onClose();}} onClick={e=>{if(e.target===ref.current)onClose();}}>{children}</dialog>;
}
