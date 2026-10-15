// Events and Component — the lifecycle backbone of the Obsidian API.

export interface EventRef {
  e: Events;
  name: string;
  fn: (...args: any[]) => any;
  ctx?: any;
}

export class Events {
  _: Record<string, EventRef[]> = {};

  on(name: string, callback: (...data: any[]) => any, ctx?: any): EventRef {
    const ref: EventRef = { e: this, name, fn: callback, ctx };
    (this._[name] ||= []).push(ref);
    return ref;
  }

  off(name: string, callback: (...data: any[]) => any): void {
    const list = this._[name];
    if (!list) return;
    this._[name] = list.filter((r) => r.fn !== callback);
  }

  offref(ref: EventRef): void {
    const list = this._[ref.name];
    if (!list) return;
    this._[ref.name] = list.filter((r) => r !== ref);
  }

  trigger(name: string, ...data: any[]): void {
    const list = this._[name];
    if (!list) return;
    for (const ref of list.slice()) this.tryTrigger(ref, data);
  }

  tryTrigger(ref: EventRef, args: any[]): void {
    try {
      ref.fn.apply(ref.ctx, args);
    } catch (e) {
      console.error(e);
    }
  }
}

export class Component {
  _loaded = false;
  _events: (() => any)[] = [];
  _children: Component[] = [];

  load(): void {
    if (this._loaded) return;
    this._loaded = true;
    try {
      this.onload();
    } catch (e) {
      console.error(e);
    }
    for (const c of this._children.slice()) c.load();
  }

  onload(): void {}

  unload(): void {
    if (!this._loaded) return;
    this._loaded = false;
    const children = this._children;
    this._children = [];
    for (const c of children) c.unload();
    const events = this._events;
    this._events = [];
    for (const fn of events) {
      try {
        fn();
      } catch (e) {
        console.error(e);
      }
    }
    try {
      this.onunload();
    } catch (e) {
      console.error(e);
    }
  }

  onunload(): void {}

  addChild<T extends Component>(component: T): T {
    this._children.push(component);
    if (this._loaded) component.load();
    return component;
  }

  removeChild<T extends Component>(component: T): T {
    const i = this._children.indexOf(component);
    if (i !== -1) {
      this._children.splice(i, 1);
      component.unload();
    }
    return component;
  }

  register(cb: () => any): void {
    this._events.push(cb);
  }

  registerEvent(ref: EventRef): void {
    this.register(() => ref.e.offref(ref));
  }

  registerDomEvent(el: any, type: string, callback: any, options?: any): void {
    el.addEventListener(type, callback, options);
    this.register(() => el.removeEventListener(type, callback, options));
  }

  registerScopeEvent(handler: any): void {
    this.register(() => handler.scope?.unregister(handler));
  }

  registerInterval(id: number): number {
    this.register(() => window.clearInterval(id));
    return id;
  }
}
