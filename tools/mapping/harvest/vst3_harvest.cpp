// vst3_harvest: dump every parameter of one .vst3 module as JSON.
//   vst3_harvest --dump /path/to/x.vst3 --out dump.json
// JSON goes to --out because plugins may write to stdout/stderr.

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>
#include <map>

#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/connectionproxy.h"
#include "public.sdk/source/vst/utility/stringconvert.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstunits.h"
#include "pluginterfaces/vst/ivstmessage.h"
#include "pluginterfaces/base/ipluginbase.h"

using namespace Steinberg;
using namespace Steinberg::Vst;

static std::string u16(const String128 s) {
    return Steinberg::Vst::StringConvert::convert(s);
}

static std::string jesc(const std::string& s) {
    std::string o;
    for (char c : s) {
        switch (c) {
            case '"': o += "\\\""; break;
            case '\\': o += "\\\\"; break;
            case '\n': o += "\\n"; break;
            case '\r': o += "\\r"; break;
            case '\t': o += "\\t"; break;
            default:
                if (static_cast<unsigned char>(c) < 0x20) {
                    char b[8]; snprintf(b, 8, "\\u%04x", c); o += b;
                } else o += c;
        }
    }
    return o;
}

static std::string trunc48(const std::string& s) {
    return s.size() > 48 ? s.substr(0, 48) : s;
}

struct UnitTree {
    std::map<UnitID, UnitInfo> by_id;
    std::string path(UnitID id) {
        std::vector<std::string> parts;
        for (int guard = 0; guard < 64; ++guard) {
            auto it = by_id.find(id);
            if (it == by_id.end()) break;
            const UnitInfo& u = it->second;
            std::string n = trunc48(u16(u.name));
            if (!n.empty()) parts.insert(parts.begin(), n);
            if (u.parentUnitId == kNoParentUnitId || u.parentUnitId == id)
                break;
            id = u.parentUnitId;
        }
        std::string out;
        for (size_t i = 0; i < parts.size(); ++i) {
            if (i) out += " > ";
            out += parts[i];
        }
        return out;
    }
};

static void flags(std::ostringstream& j, int32 f) {
    j << "[";
    const char* names[] = {"can_automate", "read_only", "wrap_around",
                           "is_list", "is_hidden"};
    int bits[] = {Vst::ParameterInfo::kCanAutomate,
                  Vst::ParameterInfo::kIsReadOnly,
                  Vst::ParameterInfo::kIsWrapAround,
                  Vst::ParameterInfo::kIsList,
                  Vst::ParameterInfo::kIsHidden};
    bool first = true;
    for (int i = 0; i < 5; ++i)
        if (f & bits[i]) {
            if (!first) j << ",";
            j << "\"" << names[i] << "\"";
            first = false;
        }
    if (f & Vst::ParameterInfo::kIsProgramChange) {
        if (!first) j << ",";
        j << "\"is_program_change\"";
        first = false;
    }
    if (f & Vst::ParameterInfo::kIsBypass) {
        if (!first) j << ",";
        j << "\"is_bypass\"";
    }
    j << "]";
}

static std::string paramString(IEditController* c, ParamID id,
                               ParamValue v) {
    String128 out{};
    if (c->getParamStringByValue(id, v, out) != kResultOk)
        return "";
    return trunc48(u16(out));
}

static void dumpClass(std::ostringstream& j, const VST3::Hosting::ClassInfo& ci,
                      VST3::Hosting::PluginFactory& factory,
                      IHostApplication* ctx) {
    j << "{\"name\":\"" << jesc(trunc48(ci.name())) << "\","
      << "\"vendor\":\"" << jesc(trunc48(ci.vendor())) << "\","
      << "\"category\":\"" << jesc(ci.category()) << "\","
      << "\"params\":[";
    IPtr<Vst::IComponent> comp =
        factory.createInstance<Vst::IComponent>(ci.ID());
    if (!comp) { j << "],\"error\":\"component create failed\"}"; return; }
    if (comp->initialize(ctx) != kResultOk) {
        j << "],\"error\":\"component init failed\"}"; return;
    }
    TUID ctrlCid{};
    IPtr<Vst::IEditController> ctrl;
    if (comp->getControllerClassId(ctrlCid) == kResultOk) {
        ctrl = factory.createInstance<Vst::IEditController>(
            VST3::UID::fromTUID(ctrlCid));
    }
    if (!ctrl)
        ctrl = FUnknownPtr<Vst::IEditController>(comp).getInterface();
    if (!ctrl) { j << "],\"error\":\"no edit controller\"}"; return; }
    if (ctrl->initialize(ctx) != kResultOk) {
        j << "],\"error\":\"controller init failed\"}"; return;
    }
    FUnknownPtr<Vst::IConnectionPoint> compCP(comp);
    FUnknownPtr<Vst::IConnectionPoint> ctrlCP(ctrl);
    if (compCP && ctrlCP) {
        compCP->connect(ctrlCP);
        ctrlCP->connect(compCP);
    }

    UnitTree units;
    FUnknownPtr<IUnitInfo> ui(ctrl.get());
    if (ui) {
        int32 n = ui->getUnitCount();
        for (int32 i = 0; i < n; ++i) {
            UnitInfo info{};
            if (ui->getUnitInfo(i, info) == kResultOk)
                units.by_id[info.id] = info;
        }
    }

    int32 n = ctrl->getParameterCount();
    for (int32 i = 0; i < n; ++i) {
        ParameterInfo info{};
        if (ctrl->getParameterInfo(i, info) != kResultOk) continue;
        if (i) j << ",";
        double def = info.defaultNormalizedValue;
        j << "{\"index\":" << i
          << ",\"id\":" << info.id
          << ",\"name\":\"" << jesc(trunc48(u16(info.title))) << "\""
          << ",\"short\":\"" << jesc(trunc48(u16(info.shortTitle)))
          << "\""
          << ",\"units\":\"" << jesc(trunc48(u16(info.units))) << "\""
          << ",\"unit_path\":\""
          << jesc(units.path(info.unitId)) << "\""
          << ",\"steps\":" << info.stepCount
          << ",\"min\":0.0,\"max\":1.0"
          << ",\"default\":" << def
          << ",\"flags\":";
        flags(j, info.flags);
        ParamValue cur = ctrl->getParamNormalized(info.id);
        j << ",\"value\":" << cur
          << ",\"samples\":[";
        if (info.stepCount > 0 && info.stepCount <= 64) {
            for (int32 st = 0; st <= info.stepCount; ++st) {
                double t = (double)st / info.stepCount;
                if (st) j << ",";
                j << "[" << t << ",\""
                  << jesc(paramString(ctrl, info.id, t)) << "\"]";
            }
        } else {
            for (int st = 0; st <= 10; ++st) {
                double t = st / 10.0;
                if (st) j << ",";
                j << "[" << t << ",\""
                  << jesc(paramString(ctrl, info.id, t)) << "\"]";
            }
        }
        j << "],\"default_string\":\""
          << jesc(paramString(ctrl, info.id, def)) << "\"}";
    }
    compCP->disconnect(ctrlCP);
    ctrlCP->disconnect(compCP);
    ctrl->terminate();
    comp->terminate();
    j << "]}";
}

int main(int argc, char** argv) {
    std::string path, out;
    for (int i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--dump") && i + 1 < argc)
            path = argv[++i];
        else if (!strcmp(argv[i], "--out") && i + 1 < argc)
            out = argv[++i];
    }
    if (path.empty()) {
        fprintf(stderr, "usage: vst3_harvest --dump <bundle> [--out f]\n");
        return 2;
    }
    std::string error;
    auto module = VST3::Hosting::Module::create(path, error);
    if (!module) {
        fprintf(stderr, "load: %s\n", error.c_str());
        return 1;
    }
    auto factory = module->getFactory();
    IPtr<Vst::HostApplication> ctx =
        IPtr<Vst::HostApplication>(new Vst::HostApplication(), true);
    std::ostringstream j;
    j << "{\"module\":\"" << jesc(path) << "\",\"classes\":[";
    int n = 0;
    for (auto& ci : factory.classInfos()) {
        if (ci.category() != kVstAudioEffectClass) continue;
        if (n++) j << ",";
        dumpClass(j, ci, factory, ctx);
    }
    j << "],\"n_audio_classes\":" << n << "}";
    std::string doc = j.str();
    if (!out.empty()) {
        std::ofstream f(out, std::ios::trunc);
        f << doc;
    } else {
        puts(doc.c_str());
    }
    return 0;
}
