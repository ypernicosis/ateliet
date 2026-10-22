// Runs inside the QuickJS engine embedded in Ateliet before any dataviewjs
// block or Templater script. It provides what those scripts expect from the
// Dataview and Templater plugins (`dv`, `tp`, Luxon's DateTime/Duration,
// `moment`) on top of a few host functions written in Rust:
//   __host(name, jsonArgs) -> json string
// Values cross the boundary as JSON with tagged objects for dates
// ({$date: ms}), durations ({$dur: {...}}) and links ({$link: {...}}).

"use strict";

const host = (name, ...args) => {
  const r = __host(name, JSON.stringify(args));
  return r === undefined || r === null || r === "" ? null : JSON.parse(r);
};

// ── values across the boundary ─────────────────────────────────────────────

function revive(v) {
  if (v === null || typeof v !== "object") return v;
  if (Array.isArray(v)) return DataArray.wrap(v.map(revive));
  if ("$date" in v) return DateTime.fromMillis(v.$date);
  if ("$dur" in v) return Duration.fromObject(v.$dur);
  if ("$link" in v) return Link.from(v.$link);
  const o = {};
  for (const k in v) o[k] = revive(v[k]);
  return o;
}

function toWire(v, depth = 0) {
  if (depth > 64) return null;
  if (v === undefined || v === null) return null;
  if (typeof v === "number" || typeof v === "string" || typeof v === "boolean") return v;
  if (typeof v === "bigint") return Number(v);
  if (typeof v === "function") return null;
  if (v instanceof DateTime) return { $date: v.toMillis() };
  if (v instanceof Date) return { $date: v.getTime() };
  if (v instanceof Duration) return { $dur: v.toObject() };
  if (v instanceof Link) return { $link: { path: v.path, display: v.display ?? null, subpath: v.subpath ?? null, embed: !!v.embed } };
  if (v instanceof Moment) return { $date: v.valueOf() };
  if (DataArray.isDataArray(v)) return v.array().map((x) => toWire(x, depth + 1));
  if (Array.isArray(v)) return v.map((x) => toWire(x, depth + 1));
  if (v instanceof FakeElement) return { $el: v.toJSON() };
  const o = {};
  for (const k of Object.keys(v)) o[k] = toWire(v[k], depth + 1);
  return o;
}

// ── Links ──────────────────────────────────────────────────────────────────

class Link {
  constructor(path, display, subpath, embed, type) {
    this.path = path;
    this.display = display ?? undefined;
    this.subpath = subpath ?? undefined;
    this.embed = !!embed;
    this.type = type || (subpath ? (String(subpath).startsWith("^") ? "block" : "header") : "file");
  }
  static from(o) {
    return new Link(o.path, o.display, o.subpath, o.embed);
  }
  static file(path, embed = false, display) {
    return new Link(path, display, undefined, embed, "file");
  }
  static header(path, header, embed = false, display) {
    return new Link(path, display, header, embed, "header");
  }
  static block(path, block, embed = false, display) {
    return new Link(path, display, "^" + String(block).replace(/^\^/, ""), embed, "block");
  }
  withPath(path) {
    return new Link(path, this.display, this.subpath, this.embed, this.type);
  }
  withDisplay(display) {
    return new Link(this.path, display, this.subpath, this.embed, this.type);
  }
  withHeader(header) {
    return Link.header(this.path, header, this.embed, this.display);
  }
  toEmbed() {
    return new Link(this.path, this.display, this.subpath, true, this.type);
  }
  fromEmbed() {
    return new Link(this.path, this.display, this.subpath, false, this.type);
  }
  toFile() {
    return Link.file(this.path, this.embed, this.display);
  }
  equals(o) {
    return o instanceof Link && o.path === this.path && o.subpath === this.subpath;
  }
  obsidianLink() {
    const t = this.path.replace(/\.md$/, "") + (this.subpath ? "#" + this.subpath : "");
    return this.display ? `${t}|${this.display}` : t;
  }
  fileName() {
    return this.path.split("/").pop().replace(/\.md$/, "");
  }
  markdown() {
    return (this.embed ? "!" : "") + "[[" + this.obsidianLink() + "]]";
  }
  toString() {
    return this.markdown();
  }
  toObject() {
    return { path: this.path, display: this.display, subpath: this.subpath, embed: this.embed, type: this.type };
  }
}

// ── Dates (a compact Luxon) ────────────────────────────────────────────────

const UNIT_ALIASES = {
  year: "years", years: "years", y: "years", quarter: "quarters", quarters: "quarters", month: "months", months: "months", M: "months",
  week: "weeks", weeks: "weeks", w: "weeks", day: "days", days: "days", d: "days", hour: "hours", hours: "hours", h: "hours",
  minute: "minutes", minutes: "minutes", m: "minutes", second: "seconds", seconds: "seconds", s: "seconds",
  millisecond: "milliseconds", milliseconds: "milliseconds", ms: "milliseconds",
};
const unit = (u) => UNIT_ALIASES[u] || u;
const PARTS = ["years", "months", "weeks", "days", "hours", "minutes", "seconds", "milliseconds"];
const MS = { years: 365 * 864e5, quarters: 91 * 864e5, months: 30 * 864e5, weeks: 7 * 864e5, days: 864e5, hours: 36e5, minutes: 6e4, seconds: 1e3, milliseconds: 1 };

function addToDate(date, obj, sign) {
  const d = new Date(date.getTime());
  const g = (k) => sign * (obj[k] || obj[k.replace(/s$/, "")] || 0);
  const months = g("years") * 12 + g("quarters") * 3 + g("months");
  if (months) {
    const day = d.getDate();
    d.setDate(1);
    d.setMonth(d.getMonth() + Math.trunc(months));
    const last = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
    d.setDate(Math.min(day, last));
  }
  const days = g("weeks") * 7 + g("days");
  if (days) d.setDate(d.getDate() + days);
  d.setTime(d.getTime() + g("hours") * 36e5 + g("minutes") * 6e4 + g("seconds") * 1e3 + g("milliseconds"));
  return d;
}

function startOfDate(date, u, weekStart = 1) {
  const d = new Date(date.getTime());
  switch (unit(u)) {
    case "years": d.setMonth(0, 1); d.setHours(0, 0, 0, 0); break;
    case "quarters": d.setMonth(Math.floor(d.getMonth() / 3) * 3, 1); d.setHours(0, 0, 0, 0); break;
    case "months": d.setDate(1); d.setHours(0, 0, 0, 0); break;
    case "weeks": d.setDate(d.getDate() - ((d.getDay() - weekStart + 7) % 7)); d.setHours(0, 0, 0, 0); break;
    case "days": d.setHours(0, 0, 0, 0); break;
    case "hours": d.setMinutes(0, 0, 0); break;
    case "minutes": d.setSeconds(0, 0); break;
    case "seconds": d.setMilliseconds(0); break;
  }
  return d;
}

function endOfDate(date, u, weekStart = 1) {
  const s = startOfDate(date, u, weekStart);
  const n = unit(u);
  const next = addToDate(s, { [n === "quarters" ? "months" : n]: n === "quarters" ? 3 : 1 }, 1);
  return new Date(next.getTime() - 1);
}

class Duration {
  constructor(o = {}) {
    for (const p of PARTS) this["_" + p] = o[p] || 0;
    if (o.quarters) this._months += o.quarters * 3;
  }
  static fromObject(o) {
    const n = {};
    for (const k in o) n[unit(k)] = o[k];
    return new Duration(n);
  }
  static fromMillis(ms) {
    return new Duration({ milliseconds: ms });
  }
  static fromISO(s) {
    const m = /^P(?:(\d+(?:\.\d+)?)Y)?(?:(\d+(?:\.\d+)?)M)?(?:(\d+(?:\.\d+)?)W)?(?:(\d+(?:\.\d+)?)D)?(?:T(?:(\d+(?:\.\d+)?)H)?(?:(\d+(?:\.\d+)?)M)?(?:(\d+(?:\.\d+)?)S)?)?$/i.exec(String(s));
    if (!m) return Duration.invalid();
    const [, y, mo, w, d, h, mi, se] = m.map((x) => (x === undefined ? 0 : Number(x)));
    return new Duration({ years: y, months: mo, weeks: w, days: d, hours: h, minutes: mi, seconds: se });
  }
  static invalid() {
    const d = new Duration();
    d.isValid = false;
    return d;
  }
  static isDuration(o) {
    return o instanceof Duration;
  }
  get years() { return this._years; }
  get months() { return this._months; }
  get weeks() { return this._weeks; }
  get days() { return this._days; }
  get hours() { return this._hours; }
  get minutes() { return this._minutes; }
  get seconds() { return this._seconds; }
  get milliseconds() { return this._milliseconds; }
  get isValid() { return this._invalid !== true; }
  set isValid(v) { this._invalid = !v; }
  toMillis() {
    return PARTS.reduce((acc, p) => acc + this["_" + p] * MS[p], 0);
  }
  valueOf() {
    return this.toMillis();
  }
  as(u) {
    return this.toMillis() / MS[unit(u)];
  }
  get(u) {
    return this["_" + unit(u)] || 0;
  }
  toObject() {
    const o = {};
    for (const p of PARTS) if (this["_" + p]) o[p] = this["_" + p];
    return o;
  }
  plus(o) {
    const d = o instanceof Duration ? o : Duration.fromObject(typeof o === "number" ? { milliseconds: o } : o);
    const n = {};
    for (const p of PARTS) n[p] = this["_" + p] + d["_" + p];
    return new Duration(n);
  }
  minus(o) {
    const d = o instanceof Duration ? o : Duration.fromObject(typeof o === "number" ? { milliseconds: o } : o);
    return this.plus(d.negate());
  }
  negate() {
    const n = {};
    for (const p of PARTS) n[p] = -this["_" + p];
    return new Duration(n);
  }
  mapUnits(fn) {
    const n = {};
    for (const p of PARTS) n[p] = fn(this["_" + p], p);
    return new Duration(n);
  }
  shiftTo(...units) {
    units = units.map(unit);
    let ms = this.toMillis();
    const n = {};
    units.forEach((u, i) => {
      const v = ms / MS[u];
      n[u] = i === units.length - 1 ? v : Math.trunc(v);
      ms -= n[u] * MS[u];
    });
    return new Duration(n);
  }
  rescale() {
    return this.shiftTo("days", "hours", "minutes", "seconds", "milliseconds").mapUnits((v) => v);
  }
  normalize() {
    return this.rescale();
  }
  set(o) {
    const n = this.toObject();
    for (const k in o) n[unit(k)] = o[k];
    return new Duration(n);
  }
  equals(o) {
    return o instanceof Duration && o.toMillis() === this.toMillis();
  }
  toHuman() {
    return host("dur_human", this.toObject());
  }
  toString() {
    return this.toHuman();
  }
  toISO() {
    const o = this;
    let s = "P";
    if (o._years) s += o._years + "Y";
    if (o._months) s += o._months + "M";
    if (o._weeks) s += o._weeks + "W";
    if (o._days) s += o._days + "D";
    if (o._hours || o._minutes || o._seconds || o._milliseconds) {
      s += "T";
      if (o._hours) s += o._hours + "H";
      if (o._minutes) s += o._minutes + "M";
      if (o._seconds || o._milliseconds) s += o._seconds + o._milliseconds / 1000 + "S";
    }
    return s === "P" ? "PT0S" : s;
  }
  toJSON() {
    return this.toISO();
  }
  toFormat(fmt) {
    return host("dur_format", this.toObject(), fmt);
  }
}

class DateTime {
  constructor(ms) {
    this._d = new Date(ms);
  }
  static fromMillis(ms) {
    return new DateTime(ms);
  }
  static fromSeconds(s) {
    return new DateTime(s * 1000);
  }
  static fromJSDate(d) {
    return new DateTime(d.getTime());
  }
  static now() {
    return new DateTime(Date.now());
  }
  static local(y, mo = 1, d = 1, h = 0, mi = 0, s = 0, ms = 0) {
    if (y === undefined) return DateTime.now();
    return new DateTime(new Date(y, mo - 1, d, h, mi, s, ms).getTime());
  }
  static utc(...a) {
    return DateTime.local(...a);
  }
  static fromObject(o = {}) {
    const n = new Date();
    return DateTime.local(o.year ?? n.getFullYear(), o.month ?? (o.year !== undefined ? 1 : n.getMonth() + 1), o.day ?? (o.year !== undefined || o.month !== undefined ? 1 : n.getDate()), o.hour || 0, o.minute || 0, o.second || 0, o.millisecond || 0);
  }
  static fromISO(s) {
    const ms = host("parse_iso", String(s));
    return ms === null ? DateTime.invalid("unparsable") : new DateTime(ms);
  }
  static fromSQL(s) {
    return DateTime.fromISO(String(s).replace(" ", "T"));
  }
  static fromFormat(s, fmt) {
    const ms = host("parse_format", String(s), String(fmt), "luxon");
    return ms === null ? DateTime.invalid("unparsable") : new DateTime(ms);
  }
  static invalid(reason) {
    const d = new DateTime(NaN);
    d.invalidReason = reason;
    return d;
  }
  static isDateTime(o) {
    return o instanceof DateTime;
  }
  static min(...ds) {
    return ds.reduce((a, b) => (a.toMillis() <= b.toMillis() ? a : b));
  }
  static max(...ds) {
    return ds.reduce((a, b) => (a.toMillis() >= b.toMillis() ? a : b));
  }
  get isValid() { return !isNaN(this._d.getTime()); }
  get year() { return this._d.getFullYear(); }
  get month() { return this._d.getMonth() + 1; }
  get day() { return this._d.getDate(); }
  get hour() { return this._d.getHours(); }
  get minute() { return this._d.getMinutes(); }
  get second() { return this._d.getSeconds(); }
  get millisecond() { return this._d.getMilliseconds(); }
  get weekday() { return this._d.getDay() === 0 ? 7 : this._d.getDay(); }
  get quarter() { return Math.floor(this._d.getMonth() / 3) + 1; }
  get daysInMonth() { return new Date(this.year, this.month, 0).getDate(); }
  get daysInYear() { return new Date(this.year, 1, 29).getMonth() === 1 ? 366 : 365; }
  get ordinal() { return Math.round((startOfDate(this._d, "days") - new Date(this.year, 0, 1)) / 864e5) + 1; }
  get weekNumber() { return Number(this.toFormat("W")); }
  get weekYear() { return Number(this.toFormat("kkkk")); }
  get ts() { return this._d.getTime(); }
  get zoneName() { return "local"; }
  get offset() { return -this._d.getTimezoneOffset(); }
  get monthLong() { return this.toFormat("MMMM"); }
  get monthShort() { return this.toFormat("MMM"); }
  get weekdayLong() { return this.toFormat("EEEE"); }
  get weekdayShort() { return this.toFormat("EEE"); }
  get c() { return { year: this.year, month: this.month, day: this.day, hour: this.hour, minute: this.minute, second: this.second, millisecond: this.millisecond }; }
  toMillis() { return this._d.getTime(); }
  toSeconds() { return this._d.getTime() / 1000; }
  toUnixInteger() { return Math.floor(this._d.getTime() / 1000); }
  valueOf() { return this._d.getTime(); }
  toJSDate() { return new Date(this._d.getTime()); }
  toObject() { return this.c; }
  toFormat(fmt) { return this.isValid ? host("format", this.toMillis(), String(fmt), "luxon") : "Invalid DateTime"; }
  toISODate() { return this.toFormat("yyyy-MM-dd"); }
  toISOTime() { return this.toFormat("HH:mm:ss.SSSZZ"); }
  toISOWeekDate() { return this.toFormat("kkkk-'W'WW-c"); }
  toISO() { return this.toFormat("yyyy-MM-dd'T'HH:mm:ss.SSSZZ"); }
  toSQLDate() { return this.toISODate(); }
  toSQL() { return this.toFormat("yyyy-MM-dd HH:mm:ss.SSS ZZ"); }
  toJSON() { return this.toISO(); }
  toString() { return this.toISO(); }
  toLocaleString(f) {
    if (f && typeof f === "object" && (f.hour !== undefined || f.timeStyle)) return this.toFormat("D t");
    return this.toFormat("D");
  }
  toRelative() {
    const diff = this.toMillis() - Date.now();
    const d = Duration.fromMillis(Math.abs(diff)).shiftTo("days", "hours", "minutes");
    const txt = d.days >= 1 ? `${Math.round(d.days)} days` : d.hours >= 1 ? `${Math.round(d.hours)} hours` : `${Math.round(d.minutes)} minutes`;
    return diff < 0 ? `${txt} ago` : `in ${txt}`;
  }
  toRelativeCalendar() {
    const days = Math.round((startOfDate(this._d, "days") - startOfDate(new Date(), "days")) / 864e5);
    if (days === 0) return "today";
    if (days === 1) return "tomorrow";
    if (days === -1) return "yesterday";
    return days > 0 ? `in ${days} days` : `${-days} days ago`;
  }
  plus(o) {
    if (typeof o === "number") return new DateTime(this.toMillis() + o);
    const obj = o instanceof Duration ? o.toObject() : Object.fromEntries(Object.entries(o).map(([k, v]) => [unit(k), v]));
    return new DateTime(addToDate(this._d, obj, 1).getTime());
  }
  minus(o) {
    if (typeof o === "number") return new DateTime(this.toMillis() - o);
    const obj = o instanceof Duration ? o.toObject() : Object.fromEntries(Object.entries(o).map(([k, v]) => [unit(k), v]));
    return new DateTime(addToDate(this._d, obj, -1).getTime());
  }
  set(o) {
    const d = new Date(this._d.getTime());
    if (o.year !== undefined) d.setFullYear(o.year);
    if (o.month !== undefined) d.setMonth(o.month - 1);
    if (o.day !== undefined) d.setDate(o.day);
    if (o.hour !== undefined) d.setHours(o.hour);
    if (o.minute !== undefined) d.setMinutes(o.minute);
    if (o.second !== undefined) d.setSeconds(o.second);
    if (o.millisecond !== undefined) d.setMilliseconds(o.millisecond);
    if (o.weekday !== undefined) d.setDate(d.getDate() + (o.weekday - (d.getDay() || 7)));
    return new DateTime(d.getTime());
  }
  startOf(u) { return new DateTime(startOfDate(this._d, u).getTime()); }
  endOf(u) { return new DateTime(endOfDate(this._d, u).getTime()); }
  hasSame(o, u) { return this.startOf(u).toMillis() === DateTime.from(o).startOf(u).toMillis(); }
  equals(o) { return o instanceof DateTime && o.toMillis() === this.toMillis(); }
  diff(o, units = "milliseconds") {
    const other = DateTime.from(o);
    const list = (Array.isArray(units) ? units : [units]).map(unit);
    if (list.length === 1 && (list[0] === "months" || list[0] === "years")) {
      let months = (this.year - other.year) * 12 + (this.month - other.month);
      if (months > 0 && other.plus({ months }).toMillis() > this.toMillis()) months--;
      if (months < 0 && other.plus({ months }).toMillis() < this.toMillis()) months++;
      const anchor = other.plus({ months });
      const frac = (this.toMillis() - anchor.toMillis()) / (30 * 864e5);
      return list[0] === "years" ? Duration.fromObject({ years: (months + frac) / 12 }) : Duration.fromObject({ months: months + frac });
    }
    return Duration.fromMillis(this.toMillis() - other.toMillis()).shiftTo(...list);
  }
  diffNow(units) { return this.diff(DateTime.now(), units); }
  setZone() { return this; }
  toLocal() { return this; }
  toUTC() { return this; }
  setLocale() { return this; }
  reconfigure() { return this; }
  static from(o) {
    if (o instanceof DateTime) return o;
    if (o instanceof Moment) return new DateTime(o.valueOf());
    if (o instanceof Date) return new DateTime(o.getTime());
    if (typeof o === "number") return new DateTime(o);
    return DateTime.fromISO(o);
  }
}

const luxon = { DateTime, Duration, Interval: undefined, Settings: { defaultLocale: "local" }, Info: {} };

// ── moment (the subset Templater scripts use) ──────────────────────────────

class Moment {
  constructor(d) {
    this._d = d;
  }
  clone() { return new Moment(new Date(this._d.getTime())); }
  isValid() { return !isNaN(this._d.getTime()); }
  valueOf() { return this._d.getTime(); }
  unix() { return Math.floor(this._d.getTime() / 1000); }
  toDate() { return new Date(this._d.getTime()); }
  toISOString() { return this.utcISO(); }
  utcISO() {
    const p = (n, w = 2) => String(n).padStart(w, "0");
    const d = this._d;
    return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())}T${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}.${p(d.getUTCMilliseconds(), 3)}Z`;
  }
  toJSON() { return this.utcISO(); }
  toString() { return this.format("ddd MMM DD YYYY HH:mm:ss [GMT]ZZ"); }
  format(fmt) {
    if (!this.isValid()) return "Invalid date";
    return host("format", this._d.getTime(), fmt === undefined ? "YYYY-MM-DDTHH:mm:ssZ" : String(fmt), "moment");
  }
  locale(l) {
    if (l === undefined) return moment.locale();
    return this;
  }
  _unit(u) {
    if (u === "D" || u === "date") return "days";
    if (u === "isoWeek" || u === "isoWeeks") return "isoWeek";
    return unit(u);
  }
  add(n, u) {
    if (typeof n === "object" && n !== null) {
      const o = n instanceof Duration ? n.toObject() : Object.fromEntries(Object.entries(n).map(([k, v]) => [unit(k), v]));
      this._d = addToDate(this._d, o, 1);
      return this;
    }
    if (typeof n === "string" && u === undefined) return this.add(Duration.fromISO(n));
    this._d = addToDate(this._d, { [this._unit(u || "ms")]: Number(n) }, 1);
    return this;
  }
  subtract(n, u) {
    if (typeof n === "object" && n !== null) {
      const o = n instanceof Duration ? n.toObject() : Object.fromEntries(Object.entries(n).map(([k, v]) => [unit(k), v]));
      this._d = addToDate(this._d, o, -1);
      return this;
    }
    this._d = addToDate(this._d, { [this._unit(u || "ms")]: Number(n) }, -1);
    return this;
  }
  startOf(u) {
    const n = this._unit(u);
    this._d = n === "isoWeek" ? startOfDate(this._d, "weeks", 1) : startOfDate(this._d, n, moment._weekStart);
    return this;
  }
  endOf(u) {
    const n = this._unit(u);
    this._d = n === "isoWeek" ? endOfDate(this._d, "weeks", 1) : endOfDate(this._d, n, moment._weekStart);
    return this;
  }
  _get(fn, setFn, v) {
    if (v === undefined) return fn(this._d);
    setFn(this._d, Number(v));
    return this;
  }
  year(v) { return this._get((d) => d.getFullYear(), (d, x) => d.setFullYear(x), v); }
  years(v) { return this.year(v); }
  month(v) {
    if (typeof v === "string") {
      const i = host("parse_format", v, "MMMM", "moment");
      if (i !== null) v = new Date(i).getMonth();
    }
    return this._get((d) => d.getMonth(), (d, x) => d.setMonth(x), v);
  }
  months(v) { return this.month(v); }
  date(v) { return this._get((d) => d.getDate(), (d, x) => d.setDate(x), v); }
  dates(v) { return this.date(v); }
  day(v) { return this._get((d) => d.getDay(), (d, x) => d.setDate(d.getDate() + (x - d.getDay())), v); }
  days(v) { return this.day(v); }
  weekday(v) { return this._get((d) => (d.getDay() - moment._weekStart + 7) % 7, (d, x) => d.setDate(d.getDate() - ((d.getDay() - moment._weekStart + 7) % 7) + x), v); }
  isoWeekday(v) { return this._get((d) => d.getDay() || 7, (d, x) => d.setDate(d.getDate() + (x - (d.getDay() || 7))), v); }
  hour(v) { return this._get((d) => d.getHours(), (d, x) => d.setHours(x), v); }
  hours(v) { return this.hour(v); }
  minute(v) { return this._get((d) => d.getMinutes(), (d, x) => d.setMinutes(x), v); }
  minutes(v) { return this.minute(v); }
  second(v) { return this._get((d) => d.getSeconds(), (d, x) => d.setSeconds(x), v); }
  seconds(v) { return this.second(v); }
  millisecond(v) { return this._get((d) => d.getMilliseconds(), (d, x) => d.setMilliseconds(x), v); }
  week(v) { return v === undefined ? Number(this.format("w")) : this.add((v - Number(this.format("w"))) * 7, "days"); }
  weeks(v) { return this.week(v); }
  isoWeek(v) { return v === undefined ? Number(this.format("W")) : this.add((v - Number(this.format("W"))) * 7, "days"); }
  isoWeeks(v) { return this.isoWeek(v); }
  quarter() { return Math.floor(this._d.getMonth() / 3) + 1; }
  dayOfYear() { return Number(this.format("DDD")); }
  daysInMonth() { return new Date(this._d.getFullYear(), this._d.getMonth() + 1, 0).getDate(); }
  get(u) {
    const n = this._unit(u);
    return { years: this.year(), months: this.month(), days: this.date(), hours: this.hour(), minutes: this.minute(), seconds: this.second(), milliseconds: this.millisecond() }[n];
  }
  set(u, v) {
    if (typeof u === "object") {
      for (const k in u) this.set(k, u[k]);
      return this;
    }
    const n = this._unit(u);
    const m = { years: "year", months: "month", days: "date", hours: "hour", minutes: "minute", seconds: "second", milliseconds: "millisecond" }[n];
    if (m) this[m](v);
    return this;
  }
  diff(o, u = "milliseconds", float = false) {
    const other = moment(o);
    const n = this._unit(u);
    let r;
    if (n === "months" || n === "years") {
      const a = DateTime.fromMillis(this.valueOf());
      r = a.diff(DateTime.fromMillis(other.valueOf()), n).get(n);
    } else {
      r = (this.valueOf() - other.valueOf()) / (n === "isoWeek" ? MS.weeks : MS[n] || 1);
    }
    return float ? r : r < 0 ? Math.ceil(r) : Math.floor(r);
  }
  isBefore(o, u) { return u ? this.clone().startOf(u).valueOf() < moment(o).startOf(u).valueOf() : this.valueOf() < moment(o).valueOf(); }
  isAfter(o, u) { return u ? this.clone().startOf(u).valueOf() > moment(o).startOf(u).valueOf() : this.valueOf() > moment(o).valueOf(); }
  isSame(o, u) { return u ? this.clone().startOf(u).valueOf() === moment(o).startOf(u).valueOf() : this.valueOf() === moment(o).valueOf(); }
  isSameOrBefore(o, u) { return this.isSame(o, u) || this.isBefore(o, u); }
  isSameOrAfter(o, u) { return this.isSame(o, u) || this.isAfter(o, u); }
  isBetween(a, b, u) { return this.isAfter(a, u) && this.isBefore(b, u); }
  isLeapYear() { const y = this.year(); return (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0; }
  fromNow() { return DateTime.fromMillis(this.valueOf()).toRelative(); }
  calendar() { return this.format("L"); }
  local() { return this; }
  utc() { return this; }
}

function moment(input, fmt, strict) {
  if (input instanceof Moment) return input.clone();
  if (input === undefined || input === null) return new Moment(new Date());
  if (input instanceof Date) return new Moment(new Date(input.getTime()));
  if (input instanceof DateTime) return new Moment(input.toJSDate());
  if (typeof input === "number") return new Moment(new Date(input));
  if (Array.isArray(input)) return new Moment(new Date(input[0], input[1] || 0, input[2] || 1, input[3] || 0, input[4] || 0, input[5] || 0));
  if (typeof input === "object") return new Moment(new Date(input.year ?? new Date().getFullYear(), input.month ?? 0, input.day ?? input.date ?? 1, input.hour || 0, input.minute || 0, input.second || 0));
  const s = String(input);
  const fmts = fmt === undefined ? [] : Array.isArray(fmt) ? fmt : [fmt];
  for (const f of fmts) {
    const ms = host("parse_format", s, String(f), "moment");
    if (ms !== null) return new Moment(new Date(ms));
  }
  if (fmts.length) return new Moment(new Date(NaN));
  const ms = host("parse_iso", s);
  return new Moment(new Date(ms === null ? NaN : ms));
}
moment.locale = (l) => host("locale");
moment.now = () => Date.now();
moment.isMoment = (o) => o instanceof Moment;
moment.isDate = (o) => o instanceof Date;
moment.unix = (s) => moment(s * 1000);
moment.duration = (n, u) => {
  if (typeof n === "object") return Duration.fromObject(n);
  if (typeof n === "string" && u === undefined) return Duration.fromISO(n);
  return Duration.fromObject({ [unit(u || "milliseconds")]: Number(n) });
};
moment.months = () => host("names", "months");
moment.monthsShort = () => host("names", "months_short");
moment.weekdays = () => host("names", "weekdays");
moment.weekdaysShort = () => host("names", "weekdays_short");
moment.weekdaysMin = () => host("names", "weekdays_min");
moment.min = (...a) => a.flat().reduce((x, y) => (x.valueOf() <= y.valueOf() ? x : y));
moment.max = (...a) => a.flat().reduce((x, y) => (x.valueOf() >= y.valueOf() ? x : y));
moment.localeData = () => ({ firstDayOfWeek: () => moment._weekStart, months: moment.months, weekdays: moment.weekdays });
moment._weekStart = 0;

// ── DataArray ──────────────────────────────────────────────────────────────

function compareValues(a, b) {
  const rank = (v) =>
    v === null || v === undefined ? 0 : typeof v === "number" ? 1 : typeof v === "string" ? 2 : typeof v === "boolean" ? 3 : v instanceof DateTime ? 4 : v instanceof Duration ? 5 : v instanceof Link ? 6 : Array.isArray(v) || DataArray.isDataArray(v) ? 7 : 8;
  const ra = rank(a), rb = rank(b);
  if (ra !== rb) return ra - rb;
  switch (ra) {
    case 0: return 0;
    case 1: return a - b;
    case 2: return a.localeCompare(b);
    case 3: return Number(a) - Number(b);
    case 4: case 5: return a.toMillis() - b.toMillis();
    case 6: return a.path.toLowerCase().localeCompare(b.path.toLowerCase());
    case 7: {
      const x = Array.from(a), y = Array.from(b);
      for (let i = 0; i < Math.min(x.length, y.length); i++) {
        const c = compareValues(x[i], y[i]);
        if (c) return c;
      }
      return x.length - y.length;
    }
    default: return 0;
  }
}

const DATA_ARRAY = Symbol("DataArray");

class DataArrayImpl {
  constructor(values, settings) {
    this.values = values;
    this.settings = settings || {};
    this[DATA_ARRAY] = true;
  }
  static wrap(arr, settings) {
    if (DataArray.isDataArray(arr)) return arr;
    return new Proxy(new DataArrayImpl(Array.isArray(arr) ? arr : Array.from(arr), settings), DataArrayImpl.HANDLER);
  }
  get length() { return this.values.length; }
  where(p) { return DataArray.wrap(this.values.filter(p)); }
  filter(p) { return this.where(p); }
  map(f) { return DataArray.wrap(this.values.map(f)); }
  flatMap(f) {
    const out = [];
    for (const v of this.values.map(f)) {
      if (Array.isArray(v) || DataArray.isDataArray(v)) out.push(...Array.from(v));
      else out.push(v);
    }
    return DataArray.wrap(out);
  }
  mutate(f) { this.values.forEach(f); return DataArray.wrap(this.values); }
  limit(n) { return DataArray.wrap(this.values.slice(0, n)); }
  slice(a, b) { return DataArray.wrap(this.values.slice(a, b)); }
  concat(o) { return DataArray.wrap(this.values.concat(Array.from(o))); }
  indexOf(e, from) { return this.values.findIndex((v, i) => i >= (from || 0) && compareValues(v, e) === 0); }
  find(p) { return this.values.find(p); }
  findIndex(p, from) { return this.values.findIndex((v, i, a) => i >= (from || 0) && p(v, i, a)); }
  includes(e) { return this.indexOf(e) !== -1; }
  join(sep = ", ") { return this.values.map((v) => (v instanceof Link ? v.markdown() : String(v))).join(sep); }
  sort(key, dir = "asc", cmp) {
    const k = key ? (typeof key === "function" ? key : (x) => x[key]) : (x) => x;
    const c = cmp || compareValues;
    const s = dir === "desc" ? -1 : 1;
    return DataArray.wrap(this.values.map((v, i) => [v, i]).sort((a, b) => s * c(k(a[0]), k(b[0])) || a[1] - b[1]).map((x) => x[0]));
  }
  sortInPlace(key, dir, cmp) {
    this.values = Array.from(this.sort(key, dir, cmp));
    return this;
  }
  groupBy(key, cmp) {
    const k = typeof key === "function" ? key : (x) => x[key];
    const c = cmp || compareValues;
    const groups = [];
    for (const v of this.values) {
      const kv = k(v);
      const g = groups.find((x) => c(x.key, kv) === 0);
      if (g) g.rows.push(v);
      else groups.push({ key: kv, rows: [v] });
    }
    groups.sort((a, b) => c(a.key, b.key));
    return DataArray.wrap(groups.map((g) => ({ key: g.key, rows: DataArray.wrap(g.rows) })));
  }
  groupIn(key, cmp) {
    return this.groupBy(key, cmp);
  }
  distinct(key, cmp) {
    const k = key ? (typeof key === "function" ? key : (x) => x[key]) : (x) => x;
    const c = cmp || compareValues;
    const out = [], keys = [];
    for (const v of this.values) {
      const kv = k(v);
      if (!keys.some((x) => c(x, kv) === 0)) {
        keys.push(kv);
        out.push(v);
      }
    }
    return DataArray.wrap(out);
  }
  every(f) { return this.values.every(f); }
  some(f) { return this.values.some(f); }
  none(f) { return !this.values.some(f); }
  first() { return this.values.length ? this.values[0] : undefined; }
  last() { return this.values.length ? this.values[this.values.length - 1] : undefined; }
  to(key) {
    const out = [];
    for (const v of this.values) {
      if (v === null || v === undefined) continue;
      const x = v instanceof Link ? (lookupLink(v) || {})[key] : v[key];
      if (x === undefined || x === null) continue;
      if (Array.isArray(x) || DataArray.isDataArray(x)) out.push(...Array.from(x));
      else out.push(x);
    }
    return DataArray.wrap(out);
  }
  into(key) {
    const out = [];
    for (const v of this.values) {
      const x = v?.[key];
      if (x === undefined || x === null) continue;
      if (Array.isArray(x) || DataArray.isDataArray(x)) out.push(...Array.from(x));
      else out.push(x);
    }
    return DataArray.wrap(out);
  }
  expand(key) {
    const out = [];
    const walk = (v) => {
      out.push(v);
      const kids = v?.[key];
      if (kids) for (const c of Array.from(kids)) walk(c);
    };
    this.values.forEach(walk);
    return DataArray.wrap(out);
  }
  forEach(f) { this.values.forEach(f); }
  array() { return this.values.slice(); }
  sum() { return this.values.reduce((a, b) => (b === null || b === undefined ? a : a + (b instanceof Duration ? b.toMillis() : Number(b))), 0); }
  avg() { return this.values.length ? this.sum() / this.values.length : NaN; }
  min() { return this.values.reduce((a, b) => (a === undefined || compareValues(b, a) < 0 ? b : a), undefined); }
  max() { return this.values.reduce((a, b) => (a === undefined || compareValues(b, a) > 0 ? b : a), undefined); }
  toString() { return this.join(", "); }
  [Symbol.iterator]() { return this.values[Symbol.iterator](); }
}

DataArrayImpl.HANDLER = {
  get(target, prop) {
    if (typeof prop === "symbol") return prop === DATA_ARRAY ? true : target[prop];
    if (/^\d+$/.test(prop)) return target.values[Number(prop)];
    if (prop in target || prop in DataArrayImpl.prototype) {
      const v = target[prop];
      return typeof v === "function" ? v.bind(target) : v;
    }
    return target.to(prop);
  },
  set(target, prop, value) {
    if (typeof prop === "string" && /^\d+$/.test(prop)) target.values[Number(prop)] = value;
    else target[prop] = value;
    return true;
  },
  has(target, prop) {
    return typeof prop === "string" && /^\d+$/.test(prop) ? Number(prop) < target.values.length : prop in target;
  },
};

const DataArray = {
  wrap: (arr, settings) => DataArrayImpl.wrap(arr, settings),
  convert: (v) => (Array.isArray(v) ? DataArray.wrap(v) : v),
  isDataArray: (v) => !!(v && typeof v === "object" && v[DATA_ARRAY] === true),
  from: (v) => DataArray.wrap(Array.from(v)),
};

function lookupLink(link) {
  if (!(link instanceof Link)) return null;
  return revive(host("page", link.path, globalThis.__origin || ""));
}

// ── a recording DOM for dv.el / dv.container ───────────────────────────────

/** A table, list, task list, query or stylesheet placed among the elements. */
class OpNode {
  constructor(op) {
    this.op = op;
  }
}

class FakeElement {
  constructor(tag, opts = {}) {
    this.tag = tag || "div";
    this.children = [];
    this.attrs = {};
    this.classes = [];
    this.style = {};
    this.text = undefined;
    this.html = undefined;
    this.markdown = undefined;
    if (typeof opts === "string") opts = { cls: opts };
    if (opts.cls) this.addClass(...(Array.isArray(opts.cls) ? opts.cls : String(opts.cls).split(/\s+/)));
    if (opts.text !== undefined) this.setText(opts.text);
    if (opts.attr) for (const k in opts.attr) this.attrs[k] = String(opts.attr[k]);
    if (opts.href) this.attrs.href = opts.href;
    if (opts.title) this.attrs.title = opts.title;
    if (opts.value !== undefined) this.attrs.value = String(opts.value);
    if (opts.type) this.attrs.type = opts.type;
    if (opts.placeholder) this.attrs.placeholder = opts.placeholder;
  }
  createEl(tag, opts, cb) {
    const el = new FakeElement(tag, opts);
    this.children.push(el);
    if (typeof cb === "function") cb(el);
    return el;
  }
  createDiv(opts, cb) { return this.createEl("div", opts, cb); }
  createSpan(opts, cb) { return this.createEl("span", opts, cb); }
  appendChild(el) { if (el instanceof FakeElement) this.children.push(el); else if (el !== undefined) this.children.push(String(el)); return el; }
  append(...els) { els.forEach((e) => this.appendChild(e)); }
  appendText(t) { this.children.push(String(t)); }
  setText(t) {
    this.children = [];
    this.text = t instanceof DateTime || t instanceof Link || t instanceof Duration ? toWire(t) : String(t);
    return this;
  }
  empty() { this.children = []; this.text = undefined; this.html = undefined; }
  addClass(...c) { for (const x of c.flat()) if (x && !this.classes.includes(x)) this.classes.push(x); return this; }
  addClasses(c) { return this.addClass(...c); }
  removeClass(...c) { this.classes = this.classes.filter((x) => !c.includes(x)); return this; }
  toggleClass(c, on) { return on ?? !this.classes.includes(c) ? this.addClass(c) : this.removeClass(c); }
  hasClass(c) { return this.classes.includes(c); }
  setAttr(k, v) { this.attrs[k] = String(v); return this; }
  setAttribute(k, v) { return this.setAttr(k, v); }
  getAttribute(k) { return this.attrs[k] ?? null; }
  removeAttribute(k) { delete this.attrs[k]; }
  setCssStyles(s) { Object.assign(this.style, s); }
  addEventListener() {}
  removeEventListener() {}
  onClickEvent() {}
  querySelector() { return null; }
  querySelectorAll() { return []; }
  set textContent(t) { this.setText(t ?? ""); }
  get textContent() { return typeof this.text === "string" ? this.text : ""; }
  set innerText(t) { this.setText(t ?? ""); }
  get innerText() { return this.textContent; }
  set innerHTML(h) { this.children = []; this.text = undefined; this.html = String(h); }
  get innerHTML() { return this.html || ""; }
  set className(c) { this.classes = String(c).split(/\s+/).filter(Boolean); }
  get className() { return this.classes.join(" "); }
  get classList() {
    const el = this;
    return { add: (...c) => el.addClass(...c), remove: (...c) => el.removeClass(...c), toggle: (c, on) => el.toggleClass(c, on), contains: (c) => el.hasClass(c) };
  }
  toJSON() {
    return {
      tag: this.tag,
      cls: this.classes,
      attrs: this.attrs,
      style: this.style,
      text: this.text,
      html: this.html,
      markdown: this.markdown,
      children: this.children.map((c) => (c instanceof FakeElement ? { $el: c.toJSON() } : c instanceof OpNode ? { $op: c.op } : c)),
    };
  }
}

// ── dv ─────────────────────────────────────────────────────────────────────

function makeDv(origin, root) {
  const push = (op) => root.children.push(new OpNode(op));
  const dv = {
    luxon,
    DateTime,
    Duration,
    Link,
    func: {},
    get container() { return root; },
    current: () => revive(host("page", origin, origin)),
    page: (path, from) => revive(host("page", path instanceof Link ? path.path : String(path), from || origin)),
    pages: (source, from) => DataArray.wrap((host("pages", source === undefined ? "" : String(source), from || origin) || []).map(revive)),
    pagePaths: (source, from) => DataArray.wrap(host("page_paths", source === undefined ? "" : String(source), from || origin) || []),
    array: (v) => DataArray.wrap(Array.isArray(v) || DataArray.isDataArray(v) ? Array.from(v) : [v]),
    isArray: (v) => Array.isArray(v) || DataArray.isDataArray(v),
    fileLink: (path, embed = false, display) => Link.file(host("resolve", String(path), origin) || String(path), embed, display),
    sectionLink: (path, section, embed = false, display) => Link.header(host("resolve", String(path), origin) || String(path), section, embed, display),
    blockLink: (path, block, embed = false, display) => Link.block(host("resolve", String(path), origin) || String(path), block, embed, display),
    date: (v) => {
      if (v instanceof DateTime) return v;
      if (v instanceof Link) return DateTime.fromISO(v.fileName());
      const w = host("date", String(v));
      return w === null ? null : DateTime.fromMillis(w);
    },
    duration: (v) => {
      if (v instanceof Duration) return v;
      const w = host("duration", String(v));
      return w === null ? null : Duration.fromObject(w);
    },
    parse: (v) => revive(host("parse_value", String(v))),
    literal: (v) => v,
    clone: (v) => revive(toWire(v)),
    compare: (a, b) => compareValues(a, b),
    equal: (a, b) => compareValues(a, b) === 0,
    io: {
      load: async (path, from) => host("load", path instanceof Link ? path.path : String(path), from || origin),
      csv: async (path, from) => {
        const text = host("load", String(path), from || origin);
        if (text === null) return undefined;
        const lines = text.split(/\r?\n/).filter((l) => l.trim());
        const split = (l) => {
          const out = [];
          let cur = "", q = false;
          for (let i = 0; i < l.length; i++) {
            const c = l[i];
            if (q) {
              if (c === '"' && l[i + 1] === '"') { cur += '"'; i++; }
              else if (c === '"') q = false;
              else cur += c;
            } else if (c === '"') q = true;
            else if (c === ",") { out.push(cur); cur = ""; }
            else cur += c;
          }
          out.push(cur);
          return out;
        };
        const head = split(lines[0] || "");
        return DataArray.wrap(lines.slice(1).map((l) => {
          const cells = split(l);
          const o = {};
          head.forEach((h, i) => (o[h] = revive(host("parse_value", cells[i] ?? ""))));
          return o;
        }));
      },
      normalize: (path, from) => host("resolve", String(path), from || origin) || String(path),
    },
    query: async (q, from) => {
      const r = host("query", String(q), from || origin);
      return r.successful ? { successful: true, value: revive(r.value) } : r;
    },
    tryQuery: async (q, from) => {
      const r = host("query", String(q), from || origin);
      if (!r.successful) throw new Error(r.error);
      return revive(r.value);
    },
    queryMarkdown: async (q, from) => host("query_markdown", String(q), from || origin),
    tryQueryMarkdown: async (q, from) => {
      const r = host("query_markdown", String(q), from || origin);
      if (!r.successful) throw new Error(r.error);
      return r.value;
    },
    evaluate: (expr, ctx) => host("evaluate", String(expr), origin),
    tryEvaluate: (expr, ctx) => {
      const r = host("evaluate", String(expr), origin);
      if (!r.successful) throw new Error(r.error);
      return revive(r.value);
    },
    // rendering
    execute: async (q) => push({ kind: "query", query: String(q) }),
    executeJs: async (code) => { await runDataviewJs(String(code), origin, root); },
    el: (tag, text, opts = {}) => {
      const el = (opts.container instanceof FakeElement ? opts.container : root).createEl(tag, { cls: opts.cls, attr: opts.attr });
      if (text !== undefined && text !== null) el.markdown = toWire(text);
      return el;
    },
    header: (level, text, opts = {}) => dv.el("h" + Math.min(6, Math.max(1, Number(level) || 1)), text, opts),
    paragraph: (text, opts = {}) => dv.el("p", text, opts),
    span: (text, opts = {}) => dv.el("span", text, opts),
    list: (items) => push({ kind: "list", items: toWire(Array.from(items || [])) }),
    table: (headers, rows) => push({ kind: "table", headers: Array.from(headers || []).map(String), rows: toWire(Array.from(rows || []).map((r) => Array.from(r))) }),
    taskList: (tasks, groupByFile = true) => push({ kind: "tasks", tasks: toWire(Array.from(tasks || [])), group: !!groupByFile }),
    view: async (path, input) => {
      const base = String(path).replace(/\.js$/, "");
      let code = host("load", base + ".js", origin);
      let css = null;
      if (code === null) {
        code = host("load", base + "/view.js", origin);
        css = host("load", base + "/view.css", origin);
      }
      if (code === null) throw new Error(`custom view not found for '${path}'`);
      if (css) push({ kind: "css", css });
      await runDataviewJs(code, origin, root, input);
    },
    markdownTable: (headers, rows) => host("markdown_table", Array.from(headers || []).map(String), toWire(Array.from(rows || []).map((r) => Array.from(r)))),
    markdownList: (items) => host("markdown_list", toWire(Array.from(items || []))),
    markdownTaskList: (tasks) => host("markdown_tasks", toWire(Array.from(tasks || []))),
    api: undefined,
  };
  dv.api = dv;
  return dv;
}

async function runDataviewJs(code, origin, root, input) {
  const dv = makeDv(origin, root);
  const fn = new AsyncFunction("dv", "input", "luxon", "moment", "app", code);
  await fn.call({ container: root, app: undefined }, dv, input, luxon, moment, undefined);
}

const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;

globalThis.__runDataviewJs = async (code, origin) => {
  globalThis.__origin = origin;
  const root = new FakeElement("div");
  await runDataviewJs(code, origin, root);
  return JSON.stringify(root.toJSON());
};

// ── Templater (`tp`) ───────────────────────────────────────────────────────

class TemplaterCancel extends Error {}

function makeTp(cfg, ops) {
  const file = { ...cfg.target };
  const fm = cfg.frontmatter || {};
  const dateFrom = (reference, referenceFormat) => {
    if (reference === undefined || reference === null || reference === "") return moment();
    return referenceFormat ? moment(reference, referenceFormat) : moment(reference);
  };
  const offsetBy = (m, offset) => {
    if (offset === undefined || offset === null || offset === "") return m;
    if (typeof offset === "number") return m.add(offset, "days");
    return m.add(Duration.fromISO(offset));
  };
  const tfile = (p) => (p ? { path: p, name: p.split("/").pop(), basename: p.split("/").pop().replace(/\.[^.]+$/, ""), extension: (p.split(".").pop() || "").toLowerCase() } : null);
  const tp = {
    file: {
      get title() { return file.basename; },
      get content() { return cfg.targetContent; },
      get tags() { return host("tp_tags", file.path) || []; },
      path: (relative = false) => (relative ? file.path : cfg.vaultPath + "/" + file.path),
      folder: (relative = false) => {
        const dir = file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/")) : "";
        return relative ? dir : dir.split("/").pop() || "";
      },
      creation_date: (fmt = "YYYY-MM-DD HH:mm") => moment(cfg.target.ctime).format(fmt),
      last_modified_date: (fmt = "YYYY-MM-DD HH:mm") => moment(cfg.target.mtime).format(fmt),
      cursor: (n) => `CURSOR${n === undefined ? 0 : n}`,
      cursor_append: (content) => {
        ops.push({ op: "cursor_append", content: String(content) });
        return "";
      },
      selection: () => cfg.selection || "",
      exists: async (p) => !!host("exists", String(p)),
      find_tfile: (name) => tfile(host("resolve", String(name).replace(/^\[\[|\]\]$/g, ""), file.path)),
      include: async (link) => {
        const target = link && link.path ? link.path : String(link).replace(/^\[\[|\]\]$/g, "");
        const resolved = host("resolve", target, file.path);
        const text = resolved ? host("load", resolved, file.path) : null;
        if (text === null) throw new Error(`Templater: cannot include ${target}`);
        return await renderTemplate(text, tp, ops);
      },
      rename: async (name) => {
        const dir = file.path.includes("/") ? file.path.slice(0, file.path.lastIndexOf("/") + 1) : "";
        file.basename = String(name);
        file.path = dir + file.basename + "." + (file.extension || "md");
        ops.push({ op: "rename", path: file.path });
      },
      move: async (newPath) => {
        let p = String(newPath).replace(/^\/+/, "");
        file.basename = p.split("/").pop();
        file.path = p + "." + (file.extension || "md");
        ops.push({ op: "rename", path: file.path });
      },
      create_new: async (template, filename, openNew = false, folder) => {
        let text = "";
        if (template && typeof template === "object" && template.path) text = host("load", template.path, file.path) || "";
        else if (typeof template === "string") {
          const r = host("resolve", template.replace(/^\[\[|\]\]$/g, ""), file.path);
          text = r && /\[\[|\.md$/.test(template) ? host("load", r, file.path) || "" : template;
        }
        const dir = folder ? (typeof folder === "string" ? folder : folder.path || "") : "";
        const name = filename ? String(filename) : "Untitled";
        const path = host("unique_path", (dir ? dir.replace(/\/+$/, "") + "/" : "") + name, "md");
        const sub = makeTp({ ...cfg, target: { ...tfile(path), ctime: Date.now(), mtime: Date.now() }, targetContent: "", frontmatter: {} }, ops);
        const content = await renderTemplate(text, sub, ops);
        host("create", path, content);
        if (openNew) ops.push({ op: "open", path });
        return tfile(path);
      },
    },
    date: {
      now: (format = "YYYY-MM-DD", offset, reference, referenceFormat) => offsetBy(dateFrom(reference, referenceFormat), offset).format(format),
      tomorrow: (format = "YYYY-MM-DD") => moment().add(1, "days").format(format),
      yesterday: (format = "YYYY-MM-DD") => moment().subtract(1, "days").format(format),
      weekday: (format = "YYYY-MM-DD", weekday = 0, reference, referenceFormat) => dateFrom(reference, referenceFormat).weekday(weekday).format(format),
    },
    frontmatter: fm,
    system: {
      prompt: async (text = "", def = "", throwOnCancel = false, multiline = false) => {
        const r = host("prompt", { kind: "prompt", text: String(text), value: def === undefined || def === null ? "" : String(def), multiline: !!multiline });
        if (r === null) {
          if (throwOnCancel) throw new TemplaterCancel("Cancelled prompt");
          return null;
        }
        return r;
      },
      suggester: async (textItems, items, throwOnCancel = false, placeholder = "", limit) => {
        const list = Array.from(items || []);
        const labels = typeof textItems === "function" ? list.map(textItems) : Array.from(textItems || []);
        const r = host("prompt", { kind: "suggester", labels: labels.map((l) => String(l)), placeholder: String(placeholder || ""), limit: limit || null });
        if (r === null || r === undefined || r < 0) {
          if (throwOnCancel) throw new TemplaterCancel("Cancelled suggester");
          return null;
        }
        return list[r];
      },
      multi_suggester: async (textItems, items, throwOnCancel = false, title = "", limit) => {
        const list = Array.from(items || []);
        const labels = typeof textItems === "function" ? list.map(textItems) : Array.from(textItems || []);
        const r = host("prompt", { kind: "multi_suggester", labels: labels.map((l) => String(l)), placeholder: String(title || ""), limit: limit || null });
        if (r === null) {
          if (throwOnCancel) throw new TemplaterCancel("Cancelled suggester");
          return [];
        }
        return r.map((i) => list[i]);
      },
      clipboard: async () => host("prompt", { kind: "clipboard" }) || "",
    },
    config: {
      template_file: tfile(cfg.templatePath),
      target_file: tfile(cfg.target.path),
      active_file: tfile(cfg.target.path),
      run_mode: cfg.runMode,
    },
    hooks: {
      on_all_templates_executed: (cb) => ops.push({ op: "hook", cb }),
    },
    web: {
      daily_quote: async () => "",
      random_picture: async () => "",
      request: async () => {
        throw new Error("tp.web.request is not available in Ateliet");
      },
    },
    user: {},
    obsidian: {},
    app: undefined,
  };
  return tp;
}

async function renderTemplate(text, tp, ops) {
  const src = host("compile_template", String(text));
  const fn = new AsyncFunction("tp", "moment", "luxon", "app", "__out", src);
  const out = (v) => (v === undefined || v === null ? "" : v instanceof Moment ? v.format() : v instanceof Link ? v.markdown() : String(v));
  return await fn(tp, moment, luxon, undefined, out);
}

globalThis.__runTemplate = async (cfgJson) => {
  const cfg = JSON.parse(cfgJson);
  moment._weekStart = cfg.weekStart || 0;
  globalThis.__origin = cfg.target.path;
  const ops = [];
  const tp = makeTp(cfg, ops);
  let text;
  try {
    text = await renderTemplate(cfg.templateText, tp, ops);
  } catch (e) {
    if (e instanceof TemplaterCancel) return JSON.stringify({ cancelled: true });
    throw e;
  }
  for (const h of ops.filter((o) => o.op === "hook")) {
    try {
      await h.cb();
    } catch (e) {
      console.error(e);
    }
  }
  return JSON.stringify({ text, ops: ops.filter((o) => o.op !== "hook") });
};

// console output goes to the app's log
globalThis.console = {
  log: (...a) => __host("log", JSON.stringify(["log", a.map(String).join(" ")])),
  info: (...a) => __host("log", JSON.stringify(["info", a.map(String).join(" ")])),
  warn: (...a) => __host("log", JSON.stringify(["warn", a.map(String).join(" ")])),
  error: (...a) => __host("log", JSON.stringify(["error", a.map(String).join(" ")])),
  debug: () => {},
};
globalThis.moment = moment;
globalThis.luxon = luxon;
globalThis.DataArray = DataArray;
globalThis.Link = Link;
